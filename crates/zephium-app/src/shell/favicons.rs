//! Profile-scoped favicon discovery, validation, caching, and persistence.

use super::*;

use zephium_ipc::IconSurface;

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

pub(super) type IconKey = (IconSurface, ProfileId, String);

/// One cached raster: the pixels chrome needs, plus the tag it caches them by.
#[derive(Clone)]
pub(super) struct IconRecord {
    pub(super) revision: String,
    pub(super) encoded: String,
}

#[derive(Default)]
pub(super) struct FaviconState {
    pub(super) icons_checked: std::collections::HashSet<(ProfileId, String)>,
    pub(super) icon_values: std::collections::HashMap<(ProfileId, String), IconRecord>,
    pub(super) icon_cache_order: std::collections::VecDeque<(ProfileId, String)>,
    /// Revision each privileged surface currently holds for an origin.
    /// Interior mutability because the paths that reference icons take `&self`.
    pub(super) delivered: std::cell::RefCell<std::collections::HashMap<IconKey, String>>,
    /// References made since the last publish whose pixels the surface lacks.
    pub(super) undelivered: std::cell::RefCell<Vec<IconKey>>,
    pub(super) icon_attempts: std::collections::HashMap<ItemId, IconAttempt>,
    pub(super) icon_load_completion_pending: std::collections::HashMap<ItemId, (ProfileId, String)>,
    pub(super) store_reads: std::collections::HashMap<ItemId, PendingFaviconStoreRead>,
    pub(super) store_generation: u64,
    pub(super) pending_batch: Option<PendingFaviconBatch>,
    pub(super) batch_generation: u64,
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

    /// Every origin the sidebar is about to draw for this window: Favourites,
    /// Pinned and Today, folder contents included.
    fn sidebar_origins(&self, profile: ProfileId, space: SpaceId) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut origins = Vec::new();
        let mut frontier: Vec<ItemId> = [
            Placement::Favorites { profile },
            Placement::Space {
                space,
                section: SpaceSection::Pinned,
            },
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        ]
        .into_iter()
        .flat_map(|placement| self.items.roots(placement).iter().copied())
        .collect();
        let mut visited = std::collections::HashSet::new();
        while let Some(id) = frontier.pop() {
            if !visited.insert(id) || origins.len() == MAX_FAVICON_BATCH_ORIGINS {
                continue;
            }
            frontier.extend(self.items.children(id).iter().copied());
            let Some(origin) = self
                .items
                .tab(id)
                .and_then(|tab| tab.url.as_ref())
                .and_then(origin_of)
            else {
                continue;
            };
            if seen.insert(origin.clone()) {
                origins.push(origin);
            }
        }
        origins
    }

    pub(super) fn hydrate_favicon_cache(&mut self, profile: ProfileId, space: SpaceId) {
        let origins = self.sidebar_origins(profile, space);
        if origins.is_empty() {
            return;
        }
        let requested: std::collections::HashSet<String> = origins.iter().cloned().collect();
        self.favicons.batch_generation = self.favicons.batch_generation.wrapping_add(1);
        if self.favicons.batch_generation == 0 {
            self.favicons.batch_generation = 1;
        }
        let generation = self.favicons.batch_generation;
        self.favicons.pending_batch = Some(PendingFaviconBatch {
            generation,
            profile,
            space,
            requested,
        });
        if let Some(reads) = &self.store_reads {
            if !reads.request_favicon_batch(generation, profile, space, origins) {
                self.favicons.pending_batch = None;
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
                self.favicons.pending_batch = None;
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
        stale: bool,
    ) {
        let exact = self.favicons.store_reads.get(&id).is_some_and(|pending| {
            pending.generation == generation
                && pending.profile == profile
                && pending.origin == origin
        });
        if !exact {
            return;
        }
        self.favicons.store_reads.remove(&id);
        if let Some(queue) = &self.self_queue {
            queue.cancel_favicon(id);
        }
        if self.item_origin(id) != Some((profile, origin.clone())) {
            return;
        }
        let key = (profile, origin.clone());
        let shown = rgba
            .as_deref()
            .is_some_and(|bytes| self.cache_icon(key.clone(), bytes));
        if shown {
            self.project_tab(id);
        }
        // A stored raster is drawn whatever its age; age only decides whether
        // the renderer is asked for a newer one.
        if stale {
            self.start_favicon_discovery(id, profile, origin);
        } else if shown {
            self.favicons.icons_checked.insert(key);
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
        let exact = self.favicons.pending_batch.as_ref().is_some_and(|pending| {
            pending.generation == generation && pending.profile == profile && pending.space == space
        });
        if !exact {
            return;
        }
        let Some(pending) = self.favicons.pending_batch.take() else {
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
        if self
            .favicons
            .icons_checked
            .contains(&(profile, origin.clone()))
        {
            return;
        }
        if self.favicons.icons_checked.len() >= TRACKED_ICON_ORIGIN_CAPACITY {
            return;
        }

        // Persistent profiles can hydrate the already-decoded fixed raster.
        // Private profiles deliberately bypass SQLite but still use the same
        // renderer-side decoder and a bounded in-memory cache.
        if self.incognito_profile(profile) {
            self.start_favicon_discovery(id, profile, origin);
            return;
        }

        let exact_pending = self
            .favicons
            .store_reads
            .get(&id)
            .is_some_and(|pending| pending.profile == profile && pending.origin == origin);
        if exact_pending {
            return;
        }
        self.cancel_favicon_attempt(id);
        self.favicons.store_generation = self.favicons.store_generation.wrapping_add(1);
        if self.favicons.store_generation == 0 {
            self.favicons.store_generation = 1;
        }
        let generation = self.favicons.store_generation;
        self.favicons.store_reads.insert(
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
            self.favicons.store_reads.remove(&id);
        } else {
            #[cfg(test)]
            {
                let stored = self.store.favicon_raster_with_age(profile, &origin);
                let stale = stored
                    .as_ref()
                    .is_none_or(|(_, age)| *age > FAVICON_CACHE_MAX_AGE_SECONDS);
                self.on_store_read(StoreReadResult::Favicon {
                    generation,
                    id,
                    profile,
                    origin,
                    rgba: stored.map(|(bytes, _)| bytes),
                    stale,
                });
                return;
            }
            #[cfg(not(test))]
            {
                self.favicons.store_reads.remove(&id);
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
            .favicons
            .icon_attempts
            .get(&id)
            .is_some_and(|attempt| attempt.profile == profile && attempt.origin == origin)
        {
            return;
        }
        if self
            .favicons
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
        self.favicons.icon_attempts.insert(
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
        self.favicons.icon_attempts.remove(&id);
        self.favicons.icon_load_completion_pending.remove(&id);
        self.favicons.store_reads.remove(&id);
        if let Some(reads) = &self.store_reads {
            reads.cancel_favicon(id);
        }
        if let Some(queue) = &self.self_queue {
            queue.cancel_favicon(id);
        }
    }

    pub(super) fn favicon_load_completed(&mut self, id: ItemId) {
        if self.favicons.icon_attempts.contains_key(&id) {
            let _ = self.engine.discover_favicon(id);
            return;
        }
        // If the pre-completion budget expired, this is its sole restart. If
        // no such marker exists, maybe_discover_favicon still respects the
        // per-origin terminal no-icon cache and therefore stays bounded under
        // duplicate load-complete notifications.
        self.favicons.icon_load_completion_pending.remove(&id);
        self.maybe_discover_favicon(id);
    }

    pub(super) fn poll_favicon(&mut self, id: ItemId, attempt: u8) {
        if attempt == 0 {
            let Some(pending) = self.favicons.store_reads.remove(&id) else {
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
        let Some(current) = self.favicons.icon_attempts.get(&id).cloned() else {
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
            if let Some(active) = self.favicons.icon_attempts.get_mut(&id) {
                active.next_attempt = next;
            }
            self.schedule_favicon_poll(id, next);
        } else {
            self.favicons.icon_attempts.remove(&id);
            if self.items.tab(id).is_some_and(|tab| tab.loading) {
                self.favicons
                    .icon_load_completion_pending
                    .insert(id, (current.profile, current.origin));
            } else {
                // A fully loaded document with no pixels has conclusively
                // consumed its bounded budget. Cache that negative result so
                // repeated completion events cannot rebuild forever.
                self.favicons.icon_load_completion_pending.remove(&id);
                self.favicons
                    .icons_checked
                    .insert((current.profile, current.origin));
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
        if self.favicons.icons_checked.len() >= TRACKED_ICON_ORIGIN_CAPACITY
            && !self.favicons.icons_checked.contains(&key)
        {
            return;
        }
        if !self.cache_icon(key.clone(), &rgba) {
            return;
        }
        self.favicons.icons_checked.insert(key);
        self.cancel_favicon_attempt(id);
        if !self.incognito_profile(profile) {
            self.store.save_favicon(
                profile,
                origin,
                Some(zephium_core::icon::RGBA32_MIME.to_owned()),
                rgba,
            );
        }
        self.project_items();
    }

    /// A Work page the agent read discovered its icon: cache it by origin
    /// exactly as a tab's would be, and hand it to chrome.
    pub(super) fn work_page_favicon(&mut self, profile: ProfileId, page_url: &str, rgba: Vec<u8>) {
        let Some(origin) = url::Url::parse(page_url)
            .ok()
            .and_then(|url| origin_of(&url))
        else {
            return;
        };
        if self.profiles.get(profile).is_none() {
            return;
        }
        self.admit_origin_icon(profile, &origin, &rgba);
    }

    /// Caches, stores and delivers one fresh raster for an origin no item
    /// names. The same capacity and age rules apply as for a tab's icon.
    pub(super) fn admit_origin_icon(
        &mut self,
        profile: ProfileId,
        origin: &str,
        rgba: &[u8],
    ) -> bool {
        let key = (profile, origin.to_owned());
        if zephium_core::icon::validated_rgba32(rgba).is_none()
            || (self.favicons.icons_checked.len() >= TRACKED_ICON_ORIGIN_CAPACITY
                && !self.favicons.icons_checked.contains(&key))
            || !self.cache_icon(key.clone(), rgba)
        {
            return false;
        }
        self.favicons.icons_checked.insert(key);
        if !self.incognito_profile(profile) {
            self.store.save_favicon(
                profile,
                origin.to_owned(),
                Some(zephium_core::icon::RGBA32_MIME.to_owned()),
                rgba.to_vec(),
            );
        }
        let _ = self.icon_ref_for(IconSurface::Chrome, profile, origin);
        self.project_items();
        true
    }

    pub(super) fn cache_icon(&mut self, key: (ProfileId, String), rgba: &[u8]) -> bool {
        let (Some(revision), Some(encoded)) = (
            zephium_core::icon::revision(rgba),
            zephium_core::icon::encode_rgba32(rgba),
        ) else {
            return false;
        };
        self.favicons
            .icon_cache_order
            .retain(|candidate| candidate != &key);
        while self.favicons.icon_values.len() >= ICON_CACHE_CAPACITY
            && !self.favicons.icon_values.contains_key(&key)
        {
            let Some(evicted) = self.favicons.icon_cache_order.pop_front() else {
                break;
            };
            self.favicons.icon_values.remove(&evicted);
            self.favicons
                .delivered
                .borrow_mut()
                .retain(|(_, profile, origin), _| {
                    (*profile, origin.as_str()) != (evicted.0, evicted.1.as_str())
                });
            // `icons_checked` also carries terminal negative results. A
            // positive entry that leaves the bounded raster cache must lose
            // only its positive terminal marker so a later visit may hydrate
            // it from SQLite (or rediscover it for a private profile).
            self.favicons.icons_checked.remove(&evicted);
        }
        self.favicons
            .icon_values
            .insert(key.clone(), IconRecord { revision, encoded });
        self.favicons.icon_cache_order.push_back(key);
        true
    }

    /// Sends each surface the pixels behind every icon reference it does not
    /// already hold at the current revision. Projections name icons rather than
    /// carrying them, so this is the one path rasters travel.
    pub(super) fn publish_icons(&self) {
        let pending = std::mem::take(&mut *self.favicons.undelivered.borrow_mut());
        if pending.is_empty() {
            return;
        }
        let mut delivered = self.favicons.delivered.borrow_mut();
        let mut grouped: std::collections::HashMap<
            (IconSurface, ProfileId),
            Vec<zephium_ipc::FaviconEntry>,
        > = std::collections::HashMap::new();
        for key in pending {
            let Some(record) = self.favicons.icon_values.get(&(key.1, key.2.clone())) else {
                continue;
            };
            if delivered.get(&key) == Some(&record.revision) {
                continue;
            }
            delivered.insert(key.clone(), record.revision.clone());
            grouped
                .entry((key.0, key.1))
                .or_default()
                .push(zephium_ipc::FaviconEntry {
                    origin: key.2,
                    revision: record.revision.clone(),
                    rgba: record.encoded.clone(),
                });
        }
        drop(delivered);
        for ((surface, profile), entries) in grouped {
            (self.emit)(Projection::Favicons(zephium_ipc::FaviconsView {
                surface,
                profile_id: profile.to_string(),
                entries,
            }));
        }
    }

    /// Names the cached icon for an origin and queues its pixels when the
    /// surface does not already hold that exact revision.
    pub(super) fn icon_ref_for(
        &self,
        surface: IconSurface,
        profile: ProfileId,
        origin: &str,
    ) -> Option<zephium_ipc::IconRef> {
        let record = self
            .favicons
            .icon_values
            .get(&(profile, origin.to_owned()))?;
        let key = (surface, profile, origin.to_owned());
        if self.favicons.delivered.borrow().get(&key) != Some(&record.revision) {
            let mut undelivered = self.favicons.undelivered.borrow_mut();
            if !undelivered.contains(&key) {
                undelivered.push(key);
            }
        }
        Some(zephium_ipc::IconRef {
            origin: origin.to_owned(),
            revision: record.revision.clone(),
        })
    }

    /// A surface reattached with an empty raster cache, so nothing it
    /// previously received can be assumed present.
    pub(super) fn forget_delivered_icons(&self, surface: IconSurface) {
        self.favicons
            .delivered
            .borrow_mut()
            .retain(|(owner, _, _), _| *owner != surface);
    }

    pub(super) fn incognito_profile(&self, profile: ProfileId) -> bool {
        self.profiles
            .get(profile)
            .is_some_and(|profile| profile.kind == ProfileKind::Incognito)
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
