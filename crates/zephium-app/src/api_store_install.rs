//! In-process, actor-issued context and exactly-once store-package handoff.
use std::sync::{Arc, Mutex};
use zephium_core::{
    ids::{ItemId, ProfileId},
    ports::{
        engine::NavigationPresentationId,
        extensions::{
            ExtensionStorePackagePreparationCallback, ExtensionStorePackagePreparationOutcome,
            ExtensionStorePackageRequest,
        },
    },
};

/// Routing context for one foreground store page. Only Shell constructs it;
/// it is revalidated after downloading and again before presenting consent.
#[derive(Clone, Debug)]
pub struct StoreExtensionContext {
    pub(crate) origin: StoreExtensionOrigin,
    pub(crate) profile: ProfileId,
    pub(crate) url: String,
    pub(crate) installed_version: Option<String>,
    pub(crate) deadline: std::time::Instant,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StoreExtensionOrigin {
    Listing {
        tab: ItemId,
        navigation: NavigationPresentationId,
    },
    Update(zephium_core::ports::extensions::ExtensionInstallSelector),
    AutomaticUpdate(
        u64,
        zephium_core::ports::extensions::ExtensionInstallSelector,
    ),
}
impl StoreExtensionContext {
    /// Canonical store listing for the native downloader. Not a package URL.
    pub fn listing_url(&self) -> &str {
        &self.url
    }
    /// Authenticated installed-package version for a conditional source check.
    pub fn installed_version(&self) -> Option<&str> {
        self.installed_version.as_deref()
    }
    /// Exact installed revision when this is a privileged update request.
    pub fn update_selector(
        &self,
    ) -> Option<zephium_core::ports::extensions::ExtensionInstallSelector> {
        match self.origin {
            StoreExtensionOrigin::Update(selector)
            | StoreExtensionOrigin::AutomaticUpdate(_, selector) => Some(selector),
            StoreExtensionOrigin::Listing { .. } => None,
        }
    }
    /// Shared absolute download/preparation deadline.
    pub const fn deadline(&self) -> std::time::Instant {
        self.deadline
    }
    pub fn is_automatic_update(&self) -> bool {
        matches!(self.origin, StoreExtensionOrigin::AutomaticUpdate(..))
    }
}

/// Non-authorizing transport result; the service independently owns every
/// package/grant/native mutation. No provider response is a trust checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreExtensionUpdateResult {
    NoChange,
    Updated,
    ReviewRequired,
    RetryLater,
    Skipped,
    FailedClosed,
}

/// Desktop-only async acquisition seam. Its callback must return immediately;
/// the existing Shell heartbeat owns scheduling, not the downloader.
type StoreUpdateJob =
    dyn Fn(StoreExtensionContext, Box<dyn FnOnce(StoreExtensionUpdateResult) + Send>) + Send + Sync;

#[derive(Clone)]
pub struct StoreExtensionUpdateDispatch {
    pub(crate) seed: u64,
    pub(crate) saved_schedule: Option<String>,
    pub(crate) run: Arc<StoreUpdateJob>,
}
impl StoreExtensionUpdateDispatch {
    pub fn new(
        seed: u64,
        saved_schedule: Option<String>,
        run: impl Fn(StoreExtensionContext, Box<dyn FnOnce(StoreExtensionUpdateResult) + Send>)
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            seed,
            saved_schedule: saved_schedule.filter(|value| value.len() <= 32),
            run: Arc::new(run),
        }
    }
}
impl std::fmt::Debug for StoreExtensionUpdateDispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreExtensionUpdateDispatch")
            .finish_non_exhaustive()
    }
}

/// Shared one-shot slot; Command clones never duplicate package bytes or callbacks.
#[derive(Clone)]
pub struct StoreExtensionPackageSubmission {
    inner: Arc<
        Mutex<
            Option<(
                StoreExtensionContext,
                ExtensionStorePackageRequest,
                ExtensionStorePackagePreparationCallback,
            )>,
        >,
    >,
}
impl StoreExtensionPackageSubmission {
    pub(crate) fn new(
        context: StoreExtensionContext,
        request: ExtensionStorePackageRequest,
        done: ExtensionStorePackagePreparationCallback,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some((context, request, done)))),
        }
    }
    pub(crate) fn take(
        &self,
    ) -> Option<(
        StoreExtensionContext,
        ExtensionStorePackageRequest,
        ExtensionStorePackagePreparationCallback,
    )> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
    pub(crate) fn settle_unavailable(&self) {
        if let Some((_, _, done)) = self.take() {
            done(ExtensionStorePackagePreparationOutcome::Unavailable);
        }
    }
}
/// One-shot completion returning from the service to the application actor.
#[derive(Clone)]
pub struct StoreExtensionPreparationCompletion {
    inner: Arc<
        Mutex<
            Option<(
                StoreExtensionContext,
                ExtensionStorePackagePreparationOutcome,
                ExtensionStorePackagePreparationCallback,
            )>,
        >,
    >,
}
impl StoreExtensionPreparationCompletion {
    pub(crate) fn new(
        context: StoreExtensionContext,
        outcome: ExtensionStorePackagePreparationOutcome,
        done: ExtensionStorePackagePreparationCallback,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some((context, outcome, done)))),
        }
    }
    pub(crate) fn take(
        &self,
    ) -> Option<(
        StoreExtensionContext,
        ExtensionStorePackagePreparationOutcome,
        ExtensionStorePackagePreparationCallback,
    )> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
    pub(crate) fn settle_unavailable(&self) {
        if let Some((_, _, done)) = self.take() {
            done(ExtensionStorePackagePreparationOutcome::Unavailable);
        }
    }
}
macro_rules! opaque_debug { ($($name:ident),+) => { $(impl std::fmt::Debug for $name { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct(stringify!($name)).finish_non_exhaustive() } })+ }; }
opaque_debug!(
    StoreExtensionPackageSubmission,
    StoreExtensionPreparationCompletion
);
