//! Profile-scoped favicon discovery, validation, caching, and persistence.

use super::*;

pub(super) const TRACKED_ICON_ORIGIN_CAPACITY: usize = 2048;
pub(super) const ICON_CACHE_CAPACITY: usize = 512;
pub(super) const FAVICON_POLL_DELAYS: [std::time::Duration; 7] = [
    std::time::Duration::from_millis(100),
    std::time::Duration::from_millis(250),
    std::time::Duration::from_millis(500),
    std::time::Duration::from_secs(1),
    std::time::Duration::from_millis(1500),
    std::time::Duration::from_millis(2500),
    std::time::Duration::from_secs(4),
];
pub(super) const STORE_READ_RESULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Clone)]
pub(super) struct IconAttempt {
    pub(super) profile: ProfileId,
    pub(super) origin: String,
    pub(super) next_attempt: u8,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PendingFaviconStoreRead {
    pub(super) generation: u64,
    pub(super) profile: ProfileId,
    pub(super) origin: String,
}

pub(super) struct PendingFaviconBatch {
    pub(super) generation: u64,
    pub(super) profile: ProfileId,
    pub(super) space: SpaceId,
    pub(super) requested: std::collections::HashSet<String>,
}

impl Shell {
    fn item_origin(&self, id: ItemId) -> Option<(ProfileId, String)> {
        let profile = self.profile_of_item(id)?;
        let origin = self
            .items
            .tab(id)
            .and_then(|t| t.url.as_ref())
            .and_then(origin_of)?;
        Some((profile, origin))
    }

    pub(super) fn hydrate_favicon_cache(&mut self, profile: ProfileId, space: SpaceId) {
        let mut requested = std::collections::HashSet::new();
        let mut origins = Vec::new();
        for id in self.today_tabs(space) {
            let Some(origin) = self
                .items
                .tab(id)
                .and_then(|tab| tab.url.as_ref())
                .and_then(origin_of)
            else {
                continue;
            };
            if requested.insert(origin.clone()) {
                origins.push(origin);
                if origins.len() == MAX_FAVICON_BATCH_ORIGINS {
                    break;
                }
            }
        }
        if origins.is_empty() {
            return;
        }
        self.favicon_batch_generation = self.favicon_batch_generation.wrapping_add(1);
        if self.favicon_batch_generation == 0 {
            self.favicon_batch_generation = 1;
        }
        let generation = self.favicon_batch_generation;
        self.pending_favicon_batch = Some(PendingFaviconBatch {
            generation,
            profile,
            space,
            requested,
        });
        if let Some(reads) = &self.store_reads {
            if !reads.request_favicon_batch(generation, profile, space, origins) {
                self.pending_favicon_batch = None;
            }
        } else {
            #[cfg(test)]
            {
                let rasters = self.store.favicon_rasters(profile, &origins);
                self.on_store_read(StoreReadResult::FaviconBatch {
                    generation,
                    profile,
                    space,
                    origins,
                    rasters,
                });
            }
            #[cfg(not(test))]
            {
                self.pending_favicon_batch = None;
            }
        }
    }

    pub(super) fn on_favicon_read(
        &mut self,
        generation: u64,
        id: ItemId,
        profile: ProfileId,
        origin: String,
        rgba: Option<Vec<u8>>,
    ) {
        let exact = self.favicon_store_reads.get(&id).is_some_and(|pending| {
            pending.generation == generation
                && pending.profile == profile
                && pending.origin == origin
        });
        if !exact {
            return;
        }
        self.favicon_store_reads.remove(&id);
        if let Some(queue) = &self.self_queue {
            queue.cancel_favicon(id);
        }
        if self.item_origin(id) != Some((profile, origin.clone())) {
            return;
        }
        let key = (profile, origin.clone());
        if rgba
            .as_deref()
            .is_some_and(|bytes| self.cache_icon(key.clone(), bytes))
        {
            self.icons_checked.insert(key);
            self.project_tab(id);
        } else {
            self.start_favicon_discovery(id, profile, origin);
        }
    }

    pub(super) fn on_favicon_batch_read(
        &mut self,
        generation: u64,
        profile: ProfileId,
        space: SpaceId,
        origins: Vec<String>,
        rasters: Vec<(String, Vec<u8>)>,
    ) {
        let exact = self.pending_favicon_batch.as_ref().is_some_and(|pending| {
            pending.generation == generation && pending.profile == profile && pending.space == space
        });
        if !exact {
            return;
        }
        let Some(pending) = self.pending_favicon_batch.take() else {
            return;
        };
        let origin_set: std::collections::HashSet<_> = origins.iter().cloned().collect();
        if origins.len() > MAX_FAVICON_BATCH_ORIGINS
            || origin_set.len() != origins.len()
            || origin_set != pending.requested
            || self
                .spaces
                .get(space)
                .is_none_or(|candidate| candidate.profile != profile)
        {
            return;
        }

        let mut accepted = std::collections::HashSet::new();
        let mut changed = false;
        for (origin, rgba) in rasters.into_iter().take(MAX_FAVICON_BATCH_ORIGINS) {
            if origin_set.contains(&origin) && accepted.insert(origin.clone()) {
                let key = (profile, origin);
                if self.cache_icon(key, &rgba) {
                    // Batch hydration intentionally does not mark freshness:
                    // it restores an immediate sidebar image, while the next
                    // live navigation still performs the one-query age check
                    // and refreshes an old raster through the renderer.
                    changed = true;
                }
            }
        }
        if changed {
            self.project_items();
        }
    }

    pub(super) fn maybe_discover_favicon(&mut self, id: ItemId) {
        let Some((profile, origin)) = self.item_origin(id) else {
            return;
        };
        if self.icons_checked.contains(&(profile, origin.clone())) {
            return;
        }
        if self.icons_checked.len() >= TRACKED_ICON_ORIGIN_CAPACITY {
            return;
        }

        // Persistent profiles can hydrate the already-decoded fixed raster.
        // Private profiles deliberately bypass SQLite but still use the same
        // renderer-side decoder and a bounded in-memory cache.
        let incognito = self
            .profiles
            .get(profile)
            .is_some_and(|profile| profile.kind == ProfileKind::Incognito);
        if incognito {
            self.start_favicon_discovery(id, profile, origin);
            return;
        }

        let exact_pending = self
            .favicon_store_reads
            .get(&id)
            .is_some_and(|pending| pending.profile == profile && pending.origin == origin);
        if exact_pending {
            return;
        }
        self.cancel_favicon_attempt(id);
        self.favicon_store_generation = self.favicon_store_generation.wrapping_add(1);
        if self.favicon_store_generation == 0 {
            self.favicon_store_generation = 1;
        }
        let generation = self.favicon_store_generation;
        self.favicon_store_reads.insert(
            id,
            PendingFaviconStoreRead {
                generation,
                profile,
                origin: origin.clone(),
            },
        );
        if let Some(reads) = &self.store_reads {
            if reads.request_favicon(generation, id, profile, origin.clone()) {
                if let Some(queue) = &self.self_queue {
                    queue.schedule_favicon(
                        id,
                        0,
                        std::time::Instant::now() + STORE_READ_RESULT_TIMEOUT,
                    );
                }
                return;
            }
            self.favicon_store_reads.remove(&id);
        } else {
            #[cfg(test)]
            {
                let rgba = self.store.fresh_favicon_raster(
                    profile,
                    &origin,
                    FAVICON_CACHE_MAX_AGE_SECONDS,
                );
                self.on_store_read(StoreReadResult::Favicon {
                    generation,
                    id,
                    profile,
                    origin,
                    rgba,
                });
                return;
            }
            #[cfg(not(test))]
            {
                self.favicon_store_reads.remove(&id);
            }
        }
        // Store-read pressure must not make favicons permanently disappear;
        // fall back to the already-bounded renderer discovery pipeline.
        if let Some((current_profile, current_origin)) = self.item_origin(id) {
            self.start_favicon_discovery(id, current_profile, current_origin);
        }
    }

    fn start_favicon_discovery(&mut self, id: ItemId, profile: ProfileId, origin: String) {
        if self
            .icon_attempts
            .get(&id)
            .is_some_and(|attempt| attempt.profile == profile && attempt.origin == origin)
        {
            return;
        }
        if self
            .icon_load_completion_pending
            .get(&id)
            .is_some_and(|pending| pending == &(profile, origin.clone()))
        {
            // The timed budget already expired while this exact document was
            // loading. Wait for its authoritative load-complete edge instead
            // of letting same-origin URL callbacks restart an unbounded loop.
            return;
        }
        self.cancel_favicon_attempt(id);
        self.icon_attempts.insert(
            id,
            IconAttempt {
                profile,
                origin,
                next_attempt: 1,
            },
        );
        let _ = self.engine.discover_favicon(id);
        self.schedule_favicon_poll(id, 1);
    }

    fn schedule_favicon_poll(&self, id: ItemId, attempt: u8) {
        let Some(delay) = FAVICON_POLL_DELAYS.get(usize::from(attempt.saturating_sub(1))) else {
            return;
        };
        if let Some(queue) = &self.self_queue {
            queue.schedule_favicon(id, attempt, std::time::Instant::now() + *delay);
        }
    }

    pub(super) fn cancel_favicon_attempt(&mut self, id: ItemId) {
        self.icon_attempts.remove(&id);
        self.icon_load_completion_pending.remove(&id);
        self.favicon_store_reads.remove(&id);
        if let Some(reads) = &self.store_reads {
            reads.cancel_favicon(id);
        }
        if let Some(queue) = &self.self_queue {
            queue.cancel_favicon(id);
        }
    }

    pub(super) fn favicon_load_completed(&mut self, id: ItemId) {
        if self.icon_attempts.contains_key(&id) {
            let _ = self.engine.discover_favicon(id);
            return;
        }
        // If the pre-completion budget expired, this is its sole restart. If
        // no such marker exists, maybe_discover_favicon still respects the
        // per-origin terminal no-icon cache and therefore stays bounded under
        // duplicate load-complete notifications.
        self.icon_load_completion_pending.remove(&id);
        self.maybe_discover_favicon(id);
    }

    pub(super) fn poll_favicon(&mut self, id: ItemId, attempt: u8) {
        if attempt == 0 {
            let Some(pending) = self.favicon_store_reads.remove(&id) else {
                return;
            };
            if let Some(reads) = &self.store_reads {
                reads.cancel_favicon(id);
            }
            if self.item_origin(id) == Some((pending.profile, pending.origin.clone())) {
                self.start_favicon_discovery(id, pending.profile, pending.origin);
            }
            return;
        }
        let Some(current) = self.icon_attempts.get(&id).cloned() else {
            return;
        };
        if current.next_attempt != attempt
            || self.item_origin(id) != Some((current.profile, current.origin.clone()))
        {
            self.cancel_favicon_attempt(id);
            return;
        }

        let _ = self.engine.discover_favicon(id);
        let next = attempt.saturating_add(1);
        if usize::from(next) <= FAVICON_POLL_DELAYS.len() {
            if let Some(active) = self.icon_attempts.get_mut(&id) {
                active.next_attempt = next;
            }
            self.schedule_favicon_poll(id, next);
        } else {
            self.icon_attempts.remove(&id);
            if self.items.tab(id).is_some_and(|tab| tab.loading) {
                self.icon_load_completion_pending
                    .insert(id, (current.profile, current.origin));
            } else {
                // A fully loaded document with no pixels has conclusively
                // consumed its bounded budget. Cache that negative result so
                // repeated completion events cannot rebuild forever.
                self.icon_load_completion_pending.remove(&id);
                self.icons_checked.insert((current.profile, current.origin));
            }
        }
    }

    pub(super) fn favicon_pixels(&mut self, id: ItemId, page_url: &str, rgba: Vec<u8>) {
        let Some((profile, origin)) = self.item_origin(id) else {
            return;
        };
        let Some(source_origin) = url::Url::parse(page_url)
            .ok()
            .and_then(|url| origin_of(&url))
        else {
            return;
        };
        if source_origin != origin || zephium_core::icon::validated_rgba32(&rgba).is_none() {
            return;
        }
        let key = (profile, origin.clone());
        if self.icons_checked.len() >= TRACKED_ICON_ORIGIN_CAPACITY
            && !self.icons_checked.contains(&key)
        {
            return;
        }
        if !self.cache_icon(key.clone(), &rgba) {
            return;
        }
        self.icons_checked.insert(key);
        self.cancel_favicon_attempt(id);
        if self
            .profiles
            .get(profile)
            .is_some_and(|profile| profile.kind != ProfileKind::Incognito)
        {
            self.store.save_favicon(
                profile,
                origin,
                Some(zephium_core::icon::RGBA32_MIME.to_owned()),
                rgba,
            );
        }
        self.project_items();
    }

    pub(super) fn cache_icon(&mut self, key: (ProfileId, String), rgba: &[u8]) -> bool {
        let Some(value) = zephium_core::icon::chrome_value(rgba) else {
            return false;
        };
        self.icon_cache_order.retain(|candidate| candidate != &key);
        while self.icon_values.len() >= ICON_CACHE_CAPACITY && !self.icon_values.contains_key(&key)
        {
            let Some(evicted) = self.icon_cache_order.pop_front() else {
                break;
            };
            self.icon_values.remove(&evicted);
            // `icons_checked` also carries terminal negative results. A
            // positive entry that leaves the bounded raster cache must lose
            // only its positive terminal marker so a later visit may hydrate
            // it from SQLite (or rediscover it for a private profile).
            self.icons_checked.remove(&evicted);
        }
        self.icon_values.insert(key.clone(), value);
        self.icon_cache_order.push_back(key);
        true
    }
}

pub(super) fn origin_of(url: &url::Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    match url.origin() {
        url::Origin::Tuple(..) => Some(url.origin().ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}
