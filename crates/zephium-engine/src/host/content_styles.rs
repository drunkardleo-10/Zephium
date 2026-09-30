//! Document-scoped, bounded style delivery. No page-to-native bridge.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use sha2::{Digest, Sha256};
use zephium_core::blocker::{BlockerSite, ContentRuleDigest};
use zephium_core::ids::{ItemId, ProfileId};

use super::dispatch::with_document_style;
use super::permits::EventPermit;
use super::EngineHost;

pub(super) type SitePreferencesSlot =
    Rc<RefCell<Option<Arc<zephium_core::blocker::PreparedBlockerSites>>>>;

pub(super) struct ViewSiteScope {
    preferences: SitePreferencesSlot,
    target: RefCell<String>,
    pub(super) pause: crate::platform::content_pause::ContentPause,
}

impl ViewSiteScope {
    pub(super) fn new(preferences: SitePreferencesSlot, url: &str) -> Rc<Self> {
        let scope = Rc::new(Self {
            preferences,
            target: RefCell::new(url.to_owned()),
            pause: Default::default(),
        });
        scope.refresh();
        scope
    }
    pub(super) fn navigating(&self, url: &str) {
        self.target.replace(url.to_owned());
        self.refresh();
    }
    pub(super) fn refresh(&self) {
        let paused = {
            let preferences = self.preferences.borrow();
            match preferences.as_ref() {
                None => true,
                Some(preferences) => BlockerSite::from_url(&self.target.borrow())
                    .and_then(|site| preferences.get(&site))
                    .is_some_and(|entry| entry.paused),
            }
        };
        self.pause.set(paused);
    }
}
use crate::navigation_epoch::{NavigationEpoch, NavigationEpochTracker};

const INSPECT: &str = "(()=>{const a=globalThis.__zephium_content_style_v1__;return a&&a.version===1?a.inspectEncoded():null})()";
const MAX_DELIVERY_BYTES: usize = 64 * 1024 * 1024;
static DELIVERY_BYTES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, PartialEq, Eq)]
struct StyleKey {
    epoch: NavigationEpoch,
    url: String,
    subscription: Option<ContentRuleDigest>,
    personal: Option<ContentRuleDigest>,
    paused: bool,
    #[cfg(target_os = "macos")]
    native_cosmetics: Option<[u8; 32]>,
}

#[derive(Default)]
pub(super) struct DocumentStyleState(Mutex<StyleState>);

#[derive(Default)]
struct StyleState {
    sequence: u64,
    active: Option<u64>,
    pending_key: Option<StyleKey>,
    applied_key: Option<StyleKey>,
    dirty: bool,
    in_flight: usize,
}

impl DocumentStyleState {
    fn lock(&self) -> MutexGuard<'_, StyleState> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

struct Delivery {
    state: Arc<DocumentStyleState>,
    sequence: u64,
    charged_bytes: usize,
    key: StyleKey,
    navigation: NavigationEpochTracker,
    permit: EventPermit,
    subscription: Arc<str>,
    personal: Arc<str>,
    #[cfg(target_os = "macos")]
    native_hint: Option<[u8; 32]>,
}

impl Drop for Delivery {
    fn drop(&mut self) {
        DELIVERY_BYTES.fetch_sub(self.charged_bytes, Ordering::Relaxed);
        let mut state = self.state.lock();
        state.in_flight -= 1;
        if state.active == Some(self.sequence) {
            state.active = None;
            state.pending_key = None;
        }
    }
}

struct DocumentIdentity {
    token: String,
    url: String,
}

impl EngineHost {
    pub(crate) fn set_blocker_site_preferences(
        &mut self,
        profile: ProfileId,
        preferences: Arc<zephium_core::blocker::PreparedBlockerSites>,
    ) -> bool {
        if self.erasure_tombstones.contains(&profile)
            || (!self.blocker_sites.contains_key(&profile)
                && self.blocker_sites.len() >= zephium_core::session::MAX_SESSION_PROFILES)
        {
            return false;
        }
        let slot = self.blocker_sites.entry(profile).or_default();
        if slot.borrow().as_ref().is_some_and(|current| {
            current.revision() > preferences.revision()
                || (current.revision() == preferences.revision()
                    && !current.same_content(&preferences))
        }) {
            return false;
        }
        slot.replace(Some(preferences));
        for (id, view) in &self.views {
            if self
                .partitions
                .get(id)
                .is_some_and(|partition| partition.profile() == profile)
            {
                view.site_scope.refresh();
            }
        }
        if let Some(spare) = self
            .spare
            .as_ref()
            .filter(|spare| spare.partition.profile() == profile)
        {
            spare.view.site_scope.refresh();
        }
        self.refresh_profile_document_styles(profile);
        true
    }
    pub(super) fn refresh_profile_document_styles(&mut self, profile: ProfileId) {
        let ids: Vec<_> = self
            .partitions
            .iter()
            .filter_map(|(id, p)| (p.profile() == profile).then_some(*id))
            .collect();
        for id in ids {
            self.refresh_document_styles(id);
        }
    }

    pub(super) fn refresh_document_styles(&mut self, id: ItemId) {
        #[cfg(target_os = "windows")]
        if self.dormant.contains(&id) || self.suspending.contains(&id) {
            return;
        }
        let Some(profile) = self.partitions.get(&id).map(|p| p.profile()) else {
            return;
        };
        let Some(sites) = self
            .blocker_sites
            .get(&profile)
            .and_then(|slot| slot.borrow().clone())
        else {
            return;
        };
        let Some(view) = self.views.get(&id) else {
            return;
        };
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let provider = self
                .content_policies
                .get(&profile)
                .and_then(|p| p.applied.as_ref())
                .and_then(|p| p.cosmetics.clone());
            view.frame_style_source.update(provider);
            if let Some(frames) = &view.frame_styles {
                frames.refresh();
            }
        }
        let Some((epoch, url)) = view.navigation.committed_snapshot() else {
            return;
        };
        let Some(site) = BlockerSite::from_url(&url) else {
            return;
        };
        let site_policy = sites.get(&site);
        let paused = site_policy.is_some_and(|p| p.paused);
        let provider = self
            .content_policies
            .get(&profile)
            .and_then(|p| p.applied.as_ref())
            .and_then(|p| p.cosmetics.as_ref());
        let key = StyleKey {
            epoch,
            url: url.clone(),
            subscription: provider.map(|p| p.fingerprint()),
            personal: site_policy.map(|p| p.fingerprint),
            paused,
            #[cfg(target_os = "macos")]
            native_cosmetics: self.native_cosmetic_digest(profile),
        };
        let state = view.content_styles.clone();
        {
            let mut status = state.lock();
            if status.applied_key.as_ref() == Some(&key) {
                return;
            }
            if status.active.is_some() {
                let replaced_document = status
                    .pending_key
                    .as_ref()
                    .is_some_and(|pending| pending.epoch != epoch);
                if !replaced_document || status.in_flight >= 2 {
                    if status.pending_key.as_ref() != Some(&key) {
                        status.dirty = true;
                    }
                    return;
                }
            }
        }
        let subscription: Arc<str> = if paused {
            Arc::from("")
        } else {
            match provider.map(|p| p.stylesheet(&url)).transpose() {
                Ok(Some(css)) => css,
                Ok(None) => Arc::from(""),
                Err(_) => return,
            }
        };
        let personal = site_policy
            .map(|p| p.css.clone())
            .unwrap_or_else(|| Arc::from(""));
        let sequence = {
            let status = state.lock();
            if subscription.is_empty() && personal.is_empty() && status.applied_key.is_none() {
                return;
            }
            let Some(sequence) = status.sequence.checked_add(1) else {
                return;
            };
            sequence
        };
        // Bound application-owned strings through both native callback phases.
        // Old documents retain their physical charge until callback/drop proof.
        let charged_bytes = (subscription.len() + personal.len())
            .saturating_mul(4)
            .saturating_add(64 * 1024);
        if DELIVERY_BYTES
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |bytes| {
                bytes
                    .checked_add(charged_bytes)
                    .filter(|total| *total <= MAX_DELIVERY_BYTES)
            })
            .is_err()
        {
            return;
        }
        {
            let mut status = state.lock();
            status.sequence = sequence;
            status.active = Some(sequence);
            status.pending_key = Some(key.clone());
            status.dirty = false;
            status.in_flight += 1;
        }
        let delivery = Delivery {
            state,
            sequence,
            charged_bytes,
            key,
            subscription,
            personal,
            navigation: view.navigation.clone(),
            permit: view.event_permit.clone(),
            #[cfg(target_os = "macos")]
            native_hint: view.native_style_document.committed.get(),
        };
        // Wry's callback is Send + Fn, not FnOnce. Contain duplicate callbacks
        // and retain exactly one owner without holding a lock across native work.
        let completion = Mutex::new(Some(delivery));
        let _ = view.evaluate_script_with_callback(INSPECT, move |result| {
            let Some(delivery) = completion.lock().unwrap_or_else(|p| p.into_inner()).take() else {
                return;
            };
            if result.len() > 72 * 1024
                || !delivery
                    .navigation
                    .matches_committed_snapshot(delivery.key.epoch, &delivery.key.url)
            {
                return;
            }
            let Some(identity) = serde_json::from_str::<Option<String>>(&result)
                .ok()
                .flatten()
                .filter(|s| s.len() <= 36 * 1024)
                .and_then(|s| decode_identity(&s))
            else {
                return;
            };
            if identity.url != delivery.key.url
                || identity.token.len() != 32
                || !identity.token.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return;
            }
            let _ = with_document_style(id, move |host| {
                host.deliver_document_styles(id, delivery, identity)
            });
        });
    }

    fn deliver_document_styles(
        &mut self,
        id: ItemId,
        delivery: Delivery,
        identity: DocumentIdentity,
    ) {
        let restart = {
            let mut state = delivery.state.lock();
            if state.active != Some(delivery.sequence) {
                return;
            }
            std::mem::take(&mut state.dirty)
        };
        if restart {
            drop(delivery);
            self.refresh_document_styles(id);
            return;
        }
        let Some(view) = self.views.get(&id) else {
            return;
        };
        if !super::permits::navigation_callback_matches(
            &view.event_permit,
            &view.navigation,
            &delivery.permit,
            &delivery.navigation,
            delivery.key.epoch,
        ) || !delivery
            .navigation
            .matches_committed_snapshot(delivery.key.epoch, &delivery.key.url)
        {
            return;
        }
        #[cfg(target_os = "macos")]
        let native = (
            delivery.native_hint.map(digest_text),
            delivery.key.native_cosmetics.map(digest_text),
        );
        #[cfg(not(target_os = "macos"))]
        let native: (Option<String>, Option<String>) = (None, None);
        let arguments = serde_json::json!([
            identity.token,
            delivery.key.url,
            format!("{:016x}", delivery.sequence),
            css_digest(&delivery.subscription),
            delivery.subscription.as_ref(),
            css_digest(&delivery.personal),
            delivery.personal.as_ref(),
            native.0,
            native.1
        ]);
        let script = format!("((p)=>{{const a=globalThis.__zephium_content_style_v1__;if(!a)return false;const covered=a.nativeCoverage?.(p[0],p[1],p[7],p[8])===true;const s=a.apply('subscription',p[0],p[1],p[2],p[3],covered?'':p[4]);const u=a.apply('personal',p[0],p[1],p[2],p[5],p[6]);return s===true&&u===true;}})({arguments})");
        if script.len() > 4 * 1024 * 1024 {
            return;
        }
        let completion = Mutex::new(Some(delivery));
        let _ = view.evaluate_script_with_callback(&script, move |result| {
            let Some(delivery) = completion.lock().unwrap_or_else(|p| p.into_inner()).take() else {
                return;
            };
            let dirty = {
                let mut state = delivery.state.lock();
                if state.active != Some(delivery.sequence) {
                    return;
                }
                if result == "true"
                    && delivery
                        .navigation
                        .matches_committed_snapshot(delivery.key.epoch, &delivery.key.url)
                {
                    state.applied_key = Some(delivery.key.clone());
                }
                std::mem::take(&mut state.dirty)
            };
            drop(delivery);
            if dirty {
                let _ = with_document_style(id, move |host| host.refresh_document_styles(id));
            }
        });
    }
}

fn css_digest(css: &str) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(css.as_bytes()) {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn decode_identity(text: &str) -> Option<DocumentIdentity> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    if object.len() != 2 {
        return None;
    }
    Some(DocumentIdentity {
        token: object.get("token")?.as_str()?.to_owned(),
        url: object.get("url")?.as_str()?.to_owned(),
    })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(PartialEq, Eq)]
struct FrameStyleKey {
    provider: Option<ContentRuleDigest>,
    preferences: Option<u64>,
    paused: bool,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) struct FrameStyleSource {
    scope: Rc<ViewSiteScope>,
    provider: RefCell<Option<Arc<dyn zephium_core::blocker::DocumentStyleProvider>>>,
    key: RefCell<Option<FrameStyleKey>>,
    generation: std::cell::Cell<u64>,
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl FrameStyleSource {
    pub(super) fn new(
        scope: Rc<ViewSiteScope>,
        provider: Option<Arc<dyn zephium_core::blocker::DocumentStyleProvider>>,
    ) -> Rc<Self> {
        let value = Rc::new(Self {
            scope,
            provider: RefCell::new(None),
            key: RefCell::new(None),
            generation: std::cell::Cell::new(0),
        });
        value.update(provider);
        value
    }
    pub(super) fn update(
        &self,
        provider: Option<Arc<dyn zephium_core::blocker::DocumentStyleProvider>>,
    ) -> bool {
        let key = FrameStyleKey {
            provider: provider.as_ref().map(|p| p.fingerprint()),
            preferences: self
                .scope
                .preferences
                .borrow()
                .as_ref()
                .map(|p| p.revision()),
            paused: self.scope.pause.paused(),
        };
        if self.key.borrow().as_ref() == Some(&key) {
            return false;
        }
        let Some(generation) = self.generation.get().checked_add(1) else {
            return false;
        };
        self.generation.set(generation);
        self.key.replace(Some(key));
        self.provider.replace(provider);
        true
    }
    pub(super) fn lookup(
        &self,
        url: &str,
    ) -> Option<crate::platform::frame_styles::FrameStyleData> {
        let site = BlockerSite::from_url(url)?;
        let preferences = self.scope.preferences.borrow();
        let paused = self.scope.pause.paused();
        let subscription = if paused {
            Arc::from("")
        } else {
            self.provider
                .borrow()
                .as_ref()
                .map(|p| p.stylesheet(url))
                .transpose()
                .ok()?
                .unwrap_or_else(|| Arc::from(""))
        };
        let personal = preferences
            .as_ref()
            .and_then(|p| p.get(&site))
            .map(|p| p.css.clone())
            .unwrap_or_else(|| Arc::from(""));
        Some(crate::platform::frame_styles::FrameStyleData {
            generation: self.generation.get(),
            subscription,
            personal,
        })
    }
}

#[cfg(target_os = "macos")]
fn digest_text(digest: [u8; 32]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(64);
    for byte in digest {
        let _ = write!(result, "{byte:02x}");
    }
    result
}
