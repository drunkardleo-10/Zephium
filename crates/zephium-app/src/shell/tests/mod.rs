use super::*;
use std::sync::Mutex;
use zephium_core::blocker::{
    BlockerConfig, BlockerConfigRevision, ContentPolicyFailure, ContentPolicyGeneration,
    ContentRuleCoverage, ContentRuleDigest, ContentRules, NetworkDecision, NetworkRequest,
    NetworkRequestPolicy, ProfileBlockerConfig,
};
use zephium_core::ids::WindowId;
use zephium_core::ports::blocker::{
    BlockerCatalog, BlockerCatalogPhase, BlockerCatalogRefreshDispatch, BlockerCatalogSnapshot,
    BlockerCompileOutcome, BlockerCompiler, BlockerDispatch, BlockerRetirementDispatch,
    BlockerShutdownOutcome,
};
use zephium_core::ports::engine::{
    ContentScope, NavigationRequestId, UserContent, UserContentGeneration, ZoomRequestId,
};
use zephium_core::ports::store::{BlockerConfigLoadOutcome, BlockerConfigUpdateOutcome};
use zephium_core::session::{
    PersistedItem, PersistedKind, PersistedProfile, PersistedSpace, SessionState,
};

type HeldErasure = (ProfileId, Box<dyn FnOnce(ProfileDataErasureOutcome) + Send>);
type HeldBlockerUpdate = (
    ProfileId,
    BlockerConfigRevision,
    BlockerConfig,
    Box<dyn FnOnce(BlockerConfigUpdateOutcome) + Send>,
);
type HeldBlockerLoad = (ProfileId, Box<dyn FnOnce(BlockerConfigLoadOutcome) + Send>);

pub(crate) struct ImmediateAllowAllCompiler;

pub(super) fn test_catalog_snapshot() -> BlockerCatalogSnapshot {
    BlockerCatalogSnapshot {
        revision: 1,
        phase: BlockerCatalogPhase::Fresh,
        enabled_policy_terminal: false,
        package_revision: Some(1),
        package_manifest_sha256: Some([1; 32]),
        package_provenance: Some(
            zephium_core::ports::blocker::BlockerCatalogProvenance::TufRepository,
        ),
        package_created_unix: Some(1),
        package_expires_unix: Some(u64::MAX),
        package_stale: Some(false),
        source_refresh_due: false,
        source_count: Some(1),
        source_bytes: Some(1),
        candidate_revision: None,
        candidate_manifest_sha256: None,
        candidate_provenance: None,
        candidate_created_unix: None,
        candidate_expires_unix: None,
        candidate_source_count: None,
        candidate_source_bytes: None,
        installed_revision: Some(1),
        installed_manifest_sha256: Some([1; 32]),
        installed_provenance: Some(
            zephium_core::ports::blocker::BlockerCatalogProvenance::TufRepository,
        ),
        refresh_supported: true,
        source_material_epoch: 0,
        source_material_repair_pending: false,
        source_material_repair_retry_pending: false,
        activation_pending: false,
        repair_retry_pending: false,
        last_refresh_attempt_unix: Some(1),
        refresh_operation: None,
    }
}

struct ImmediateNetworkPolicy;

impl NetworkRequestPolicy for ImmediateNetworkPolicy {
    fn decide(&self, _request: &NetworkRequest<'_>) -> NetworkDecision {
        NetworkDecision::Allow
    }
}

impl BlockerCompiler for ImmediateAllowAllCompiler {
    fn compile(
        &self,
        _profile: ProfileId,
        _generation: ContentPolicyGeneration,
        config: BlockerConfig,
        done: Box<dyn FnOnce(BlockerCompileOutcome) + Send>,
    ) -> BlockerDispatch {
        let rules = if config.enabled {
            ContentRules::runtime(
                ContentRuleDigest::from_bytes([1; 32]),
                ContentRuleCoverage {
                    source_rules: 1,
                    accepted_rules: 1,
                    blocking_rule_entries: 1,
                    ..ContentRuleCoverage::default()
                },
                Arc::new(ImmediateNetworkPolicy),
            )
            .expect("test policy must be valid")
        } else {
            ContentRules::allow_all(ContentRuleDigest::from_bytes([0; 32]))
        };
        done(BlockerCompileOutcome::Compiled(rules));
        BlockerDispatch::Scheduled
    }

    fn retire_profile(
        &self,
        _profile: ProfileId,
        done: Box<dyn FnOnce() + Send>,
    ) -> BlockerRetirementDispatch {
        done();
        BlockerRetirementDispatch::Quiesced
    }

    fn shutdown_until(&self, _deadline: std::time::Instant) -> BlockerShutdownOutcome {
        BlockerShutdownOutcome::Clean
    }
}

impl BlockerCatalog for ImmediateAllowAllCompiler {
    fn maintain(&self) -> BlockerCatalogSnapshot {
        test_catalog_snapshot()
    }

    fn request_refresh(&self) -> BlockerCatalogRefreshDispatch {
        BlockerCatalogRefreshDispatch::Busy
    }
}

#[derive(Default)]
pub(super) struct FakeExtensionLifecycleState {
    pub(super) startup_calls: std::sync::atomic::AtomicUsize,
    pub(super) panic_on_startup: std::sync::atomic::AtomicBool,
    pub(super) wait_until_startup_deadline: std::sync::atomic::AtomicBool,
    pub(super) startup_outcomes: Mutex<
        std::collections::VecDeque<zephium_core::ports::extensions::ExtensionServiceStartupOutcome>,
    >,
    pub(super) retirement_calls: std::sync::atomic::AtomicUsize,
    pub(super) retirement_continuation_calls: std::sync::atomic::AtomicUsize,
    pub(super) retirement_profiles: Mutex<Vec<ProfileId>>,
    pub(super) retirement_deadlines: Mutex<Vec<std::time::Instant>>,
    pub(super) retirement_outcomes: Mutex<
        std::collections::VecDeque<
            zephium_core::ports::extensions::ExtensionProfileRetirementDisposition,
        >,
    >,
    /// Overrides whether the fake invokes the continuation, allowing tests to
    /// prove the application detects a lifecycle implementation that violates
    /// disposition/callback consistency.
    pub(super) retirement_invoke_override: Mutex<Option<bool>>,
    pub(super) panic_on_retirement: std::sync::atomic::AtomicBool,
    pub(super) panic_after_retirement_continuation: std::sync::atomic::AtomicBool,
    pub(super) shutdown_calls: std::sync::atomic::AtomicUsize,
    pub(super) panic_on_shutdown: std::sync::atomic::AtomicBool,
    pub(super) dropped_without_shutdown: std::sync::atomic::AtomicBool,
    pub(super) shutdown_outcome: Mutex<Option<ExtensionServiceShutdownOutcome>>,
    pub(super) shutdown_order: Mutex<Option<Arc<Mutex<Vec<&'static str>>>>>,
    pub(super) deadlines: Mutex<Vec<std::time::Instant>>,
}

struct FakeExtensionLifecycle {
    state: Arc<FakeExtensionLifecycleState>,
}

impl Drop for FakeExtensionLifecycle {
    fn drop(&mut self) {
        if self
            .state
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire)
            == 0
        {
            self.state
                .dropped_without_shutdown
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

impl zephium_core::ports::extensions::ExtensionServiceLifecycle for FakeExtensionLifecycle {
    fn settle_startup_until(
        &mut self,
        deadline: std::time::Instant,
    ) -> zephium_core::ports::extensions::ExtensionServiceStartupOutcome {
        self.state
            .startup_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if self
            .state
            .wait_until_startup_deadline
            .load(std::sync::atomic::Ordering::Acquire)
        {
            while std::time::Instant::now() < deadline {
                std::thread::yield_now();
            }
        }
        assert!(
            !self
                .state
                .panic_on_startup
                .load(std::sync::atomic::Ordering::Acquire),
            "injected extension startup panic"
        );
        self.state
            .startup_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(zephium_core::ports::extensions::ExtensionServiceStartupOutcome::Ready)
    }

    fn with_profile_retired_until(
        &mut self,
        profile: ProfileId,
        deadline: std::time::Instant,
        continuation: Box<dyn FnOnce() + '_>,
    ) -> zephium_core::ports::extensions::ExtensionProfileRetirementDisposition {
        self.state
            .retirement_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.state.retirement_profiles.lock().unwrap().push(profile);
        self.state
            .retirement_deadlines
            .lock()
            .unwrap()
            .push(deadline);
        assert!(
            !self
                .state
                .panic_on_retirement
                .load(std::sync::atomic::Ordering::Acquire),
            "injected extension profile-retirement panic"
        );
        let disposition = self
            .state
            .retirement_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(
                zephium_core::ports::extensions::ExtensionProfileRetirementDisposition::Continued,
            );
        let invoke = self
            .state
            .retirement_invoke_override
            .lock()
            .unwrap()
            .unwrap_or(
                disposition
                    == zephium_core::ports::extensions::ExtensionProfileRetirementDisposition::Continued,
            );
        if invoke {
            self.state
                .retirement_continuation_calls
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            continuation();
            assert!(
                !self
                    .state
                    .panic_after_retirement_continuation
                    .load(std::sync::atomic::Ordering::Acquire),
                "injected post-continuation extension profile-retirement panic"
            );
        }
        disposition
    }

    fn shutdown_until(
        self: Box<Self>,
        deadline: std::time::Instant,
    ) -> ExtensionServiceShutdownOutcome {
        self.state
            .shutdown_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.state.deadlines.lock().unwrap().push(deadline);
        if let Some(order) = self.state.shutdown_order.lock().unwrap().as_ref() {
            order.lock().unwrap().push("extensions");
        }
        assert!(
            !self
                .state
                .panic_on_shutdown
                .load(std::sync::atomic::Ordering::Acquire),
            "injected extension shutdown panic"
        );
        self.state
            .shutdown_outcome
            .lock()
            .unwrap()
            .unwrap_or(ExtensionServiceShutdownOutcome::Clean)
    }
}

pub(super) fn extension_lifecycle_with_outcome(
    outcome: ExtensionServiceShutdownOutcome,
) -> (ExtensionLifecycle, Arc<FakeExtensionLifecycleState>) {
    let state = Arc::new(FakeExtensionLifecycleState::default());
    *state.shutdown_outcome.lock().unwrap() = Some(outcome);
    (
        Box::new(FakeExtensionLifecycle {
            state: Arc::clone(&state),
        }),
        state,
    )
}

pub(super) fn clean_extension_lifecycle() -> ExtensionLifecycle {
    extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean).0
}

pub(super) fn extension_lifecycle_with_startup_outcomes(
    outcomes: impl IntoIterator<Item = zephium_core::ports::extensions::ExtensionServiceStartupOutcome>,
) -> (ExtensionLifecycle, Arc<FakeExtensionLifecycleState>) {
    let (lifecycle, state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    state.startup_outcomes.lock().unwrap().extend(outcomes);
    (lifecycle, state)
}

#[derive(Default)]
pub(crate) struct FakeEngine {
    calls: Mutex<Vec<String>>,
    warm_spare_calls: std::sync::atomic::AtomicUsize,
    navigation_requests: Mutex<Vec<NavigationRequestId>>,
    zoom_requests: Mutex<Vec<(ItemId, f64, ZoomRequestId)>>,
    shutdown_result: Mutex<Option<bool>>,
    shutdown_calls: std::sync::atomic::AtomicUsize,
    shutdown_order: Mutex<Option<Arc<Mutex<Vec<&'static str>>>>>,
    panic_on_shutdown: std::sync::atomic::AtomicBool,
    skip_shutdown_callback: std::sync::atomic::AtomicBool,
    reject_create_dispatch: std::sync::atomic::AtomicBool,
    reject_navigation_dispatch: std::sync::atomic::AtomicBool,
    reject_native_dispatch: std::sync::atomic::AtomicBool,
    unsupported_presentation: std::sync::atomic::AtomicBool,
    runtime_restart_required: std::sync::atomic::AtomicBool,
    runtime_security_advisories: Mutex<zephium_core::runtime_security::RuntimeSecurityAdvisories>,
    erasure_outcomes: Mutex<VecDeque<ProfileDataErasureOutcome>>,
    erasure_requests: Mutex<Vec<(ProfileId, Option<ExtensionNativeNamespaceScope>)>>,
    held_erasures: Mutex<Vec<HeldErasure>>,
    hold_erasures: std::sync::atomic::AtomicBool,
}

impl FakeEngine {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn log(&self, s: String) {
        self.calls.lock().unwrap().push(s);
    }
    fn warm_spare_calls(&self) -> usize {
        self.warm_spare_calls
            .load(std::sync::atomic::Ordering::Acquire)
    }
    fn last_layout(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|c| {
                c.split_once(' ')
                    .filter(|(head, _)| head.starts_with("layout@"))
                    .map(|(_, rest)| rest)
            })
            .map(|s| {
                s.split(',')
                    .filter(|x| !x.is_empty())
                    .map(Into::into)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn last_navigation_request(&self) -> NavigationRequestId {
        *self
            .navigation_requests
            .lock()
            .unwrap()
            .last()
            .expect("a navigation request must have been admitted")
    }

    fn last_zoom_request(&self) -> (ItemId, f64, ZoomRequestId) {
        *self
            .zoom_requests
            .lock()
            .unwrap()
            .last()
            .expect("a zoom request must have been admitted")
    }

    fn native_admission(&self) -> NativeDispatch {
        if self
            .reject_native_dispatch
            .load(std::sync::atomic::Ordering::Acquire)
        {
            NativeDispatch::Rejected
        } else {
            NativeDispatch::Scheduled
        }
    }

    fn push_erasure_outcomes(&self, outcomes: impl IntoIterator<Item = ProfileDataErasureOutcome>) {
        self.erasure_outcomes.lock().unwrap().extend(outcomes);
    }

    fn erasure_requests(&self) -> Vec<(ProfileId, Option<ExtensionNativeNamespaceScope>)> {
        self.erasure_requests.lock().unwrap().clone()
    }

    fn complete_held_erasure(&self, outcome: ProfileDataErasureOutcome) {
        let (_, done) = self.held_erasures.lock().unwrap().remove(0);
        done(outcome);
    }
}

impl Engine for FakeEngine {
    fn runtime_restart_required(&self) -> bool {
        self.runtime_restart_required
            .load(std::sync::atomic::Ordering::Acquire)
    }

    fn runtime_security_advisories(
        &self,
    ) -> zephium_core::runtime_security::RuntimeSecurityAdvisories {
        *self.runtime_security_advisories.lock().unwrap()
    }

    fn create_view(&self, id: ItemId, partition: Partition, url: &str, _bounds: Rect) -> bool {
        if self
            .reject_create_dispatch
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return false;
        }
        let kind = match partition {
            Partition::Default(_) => "default",
            Partition::Persistent(_) => "persistent",
            Partition::Ephemeral(_) => "ephemeral",
        };
        self.log(format!("create {id} {url} [{kind}]"));
        true
    }
    fn navigate(&self, id: ItemId, url: &str, request: NavigationRequestId) -> bool {
        if self
            .reject_navigation_dispatch
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return false;
        }
        self.navigation_requests.lock().unwrap().push(request);
        self.log(format!("navigate {id} {url}"));
        true
    }
    fn present_navigation(
        &self,
        id: ItemId,
        navigation: NavigationPresentationId,
    ) -> NativeDispatch {
        self.log(format!("present {id} {}", navigation.into_raw()));
        if self
            .unsupported_presentation
            .load(std::sync::atomic::Ordering::Acquire)
        {
            NativeDispatch::Unsupported
        } else {
            self.native_admission()
        }
    }
    fn reload(&self, id: ItemId) -> NativeDispatch {
        self.log(format!("reload {id}"));
        self.native_admission()
    }
    fn stop(&self, _id: ItemId) -> NativeDispatch {
        self.native_admission()
    }
    fn go_back(&self, _id: ItemId) -> NativeDispatch {
        self.native_admission()
    }
    fn go_forward(&self, _id: ItemId) -> NativeDispatch {
        self.native_admission()
    }
    fn close(&self, id: ItemId) -> NativeDispatch {
        self.log(format!("close {id}"));
        self.native_admission()
    }
    fn set_content(
        &self,
        window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) -> NativeDispatch {
        let ids: Vec<String> = match (tree, region) {
            (Some(t), Some(_)) => t.tabs().iter().map(|id| id.to_string()).collect(),
            _ => Vec::new(),
        };
        self.log(format!("layout@{window} {}", ids.join(",")));
        self.native_admission()
    }
    fn set_drop_indicator(&self, _window: WindowId, _zone: Option<Rect>) -> NativeDispatch {
        self.native_admission()
    }
    fn zoom(&self, id: ItemId, scale: f64, request: ZoomRequestId) -> NativeDispatch {
        self.log(format!("zoom {id} {scale}"));
        let admission = self.native_admission();
        if admission == NativeDispatch::Scheduled {
            self.zoom_requests
                .lock()
                .unwrap()
                .push((id, scale, request));
        }
        admission
    }
    fn set_muted(&self, _id: ItemId, _muted: bool) -> NativeDispatch {
        NativeDispatch::Unsupported
    }
    fn find(&self, _id: ItemId, _query: Option<&str>) -> NativeDispatch {
        NativeDispatch::Unsupported
    }
    fn capture(&self, _id: ItemId) -> NativeDispatch {
        NativeDispatch::Unsupported
    }
    fn extract_html(&self, _id: ItemId) -> NativeDispatch {
        self.native_admission()
    }
    fn discover_favicon(&self, id: ItemId) -> NativeDispatch {
        self.log(format!("discover {id}"));
        self.native_admission()
    }
    fn probe_discard_safety(&self, id: ItemId, probe: DiscardProbeId) -> bool {
        self.log(format!("probe-discard {id} {}", probe.0));
        true
    }
    fn discard_view(&self, id: ItemId, probe: DiscardProbeId) -> bool {
        self.log(format!("discard {id} {}", probe.0));
        true
    }
    fn set_dormant(&self, ids: Vec<ItemId>) {
        let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
        ids.sort();
        self.log(format!("dormant {}", ids.join(",")));
    }
    fn print(&self, _id: ItemId) -> NativeDispatch {
        self.native_admission()
    }
    fn set_user_content(
        &self,
        _scope: ContentScope,
        _generation: UserContentGeneration,
        _content: UserContent,
    ) -> NativeDispatch {
        self.native_admission()
    }
    fn set_shortcuts(&self, _shortcuts: Vec<zephium_core::ports::engine::Shortcut>) {}
    fn warm_spare(&self, _partition: Partition) {
        self.warm_spare_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
    fn install_content_rules(
        &self,
        profile: ProfileId,
        generation: ContentPolicyGeneration,
        _rules: Arc<ContentRules>,
    ) -> NativeDispatch {
        self.log(format!(
            "install-content-rules {profile} {}",
            generation.get()
        ));
        self.native_admission()
    }
    fn erase_profile_data(
        &self,
        profile: ProfileId,
        extension_native_namespace: Option<ExtensionNativeNamespaceScope>,
        done: Box<dyn FnOnce(zephium_core::ports::engine::ProfileDataErasureOutcome) + Send>,
    ) {
        self.log(format!("erase-profile {profile}"));
        self.erasure_requests
            .lock()
            .unwrap()
            .push((profile, extension_native_namespace));
        if self
            .hold_erasures
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.held_erasures.lock().unwrap().push((profile, done));
            return;
        }
        let outcome = self
            .erasure_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ProfileDataErasureOutcome::Failed);
        done(outcome);
    }
    fn shutdown(&self, done: Box<dyn FnOnce(bool) + Send>) {
        self.shutdown_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if let Some(order) = self.shutdown_order.lock().unwrap().as_ref() {
            order.lock().unwrap().push("engine");
        }
        assert!(
            !self
                .panic_on_shutdown
                .load(std::sync::atomic::Ordering::Acquire),
            "injected engine shutdown panic"
        );
        if !self
            .skip_shutdown_callback
            .load(std::sync::atomic::Ordering::Acquire)
        {
            done(self.shutdown_result.lock().unwrap().unwrap_or(true));
        }
    }
}

#[derive(Default)]
pub(crate) struct FakeStore {
    saved: Mutex<Option<SessionState>>,
    events: Mutex<Vec<&'static str>>,
    flush_result: Mutex<Option<bool>>,
    shutdown_outcome: Mutex<Option<StoreShutdownOutcome>>,
    shutdown_calls: std::sync::atomic::AtomicUsize,
    shutdown_order: Mutex<Option<Arc<Mutex<Vec<&'static str>>>>>,
    barrier_order: Mutex<Option<Arc<Mutex<Vec<&'static str>>>>>,
    panic_on_save: std::sync::atomic::AtomicBool,
    panic_on_flush: std::sync::atomic::AtomicBool,
    panic_on_shutdown: std::sync::atomic::AtomicBool,
    load_failed: Mutex<bool>,
    recovery_reason: Mutex<Option<String>>,
    degraded_profiles: Mutex<Vec<ProfileId>>,
    blocker_configs: Mutex<Option<Vec<ProfileBlockerConfig>>>,
    blocker_update_outcomes: Mutex<VecDeque<BlockerConfigUpdateOutcome>>,
    blocker_load_outcomes: Mutex<VecDeque<BlockerConfigLoadOutcome>>,
    blocker_update_on_shutdown: Mutex<Option<BlockerConfigUpdateOutcome>>,
    blocker_load_calls: std::sync::atomic::AtomicUsize,
    blocker_load_after_shutdown_calls: std::sync::atomic::AtomicUsize,
    shutdown_entered: std::sync::atomic::AtomicBool,
    held_blocker_updates: Mutex<VecDeque<HeldBlockerUpdate>>,
    held_blocker_loads: Mutex<VecDeque<HeldBlockerLoad>>,
    hold_blocker_updates: std::sync::atomic::AtomicBool,
    hold_blocker_loads: std::sync::atomic::AtomicBool,
    reject_blocker_updates: std::sync::atomic::AtomicBool,
    reject_blocker_loads: std::sync::atomic::AtomicBool,
    panic_on_load: std::sync::atomic::AtomicBool,
    load_session_calls: std::sync::atomic::AtomicUsize,
    pending_deletion_load_calls: std::sync::atomic::AtomicUsize,
    history: Vec<zephium_core::ports::store::HistoryHit>,
    history_delay_ms: std::sync::atomic::AtomicU64,
    history_started: std::sync::atomic::AtomicBool,
    visits: Mutex<Vec<String>>,
    icon_ages: Mutex<std::collections::HashMap<String, i64>>,
    icons: Mutex<Vec<(String, Vec<u8>)>>,
    reject_settings: std::sync::atomic::AtomicBool,
    pending_deletions: Mutex<Vec<PendingProfileDeletion>>,
    pending_load_failures: std::sync::atomic::AtomicUsize,
    authorize_outcomes: Mutex<VecDeque<ProfileDeletionAuthorizeOutcome>>,
    authorize_unknown_commits: std::sync::atomic::AtomicBool,
    authorized_native_namespace: Mutex<Option<ExtensionNativeNamespaceScope>>,
    authorized_sessions: Mutex<Vec<(ProfileId, SessionState)>>,
    finalize_outcomes: Mutex<VecDeque<ProfileDeletionFinalizeOutcome>>,
    finalize_unknown_completes: std::sync::atomic::AtomicBool,
}

impl FakeStore {
    fn complete_blocker_update(&self, outcome: BlockerConfigUpdateOutcome) {
        let (_, _, _, done) = self
            .held_blocker_updates
            .lock()
            .unwrap()
            .pop_front()
            .expect("one blocker update must be pending");
        done(outcome);
    }

    fn complete_blocker_load(&self, outcome: BlockerConfigLoadOutcome) {
        let (_, done) = self
            .held_blocker_loads
            .lock()
            .unwrap()
            .pop_front()
            .expect("one blocker reconciliation must be pending");
        done(outcome);
    }
}

impl Store for FakeStore {
    fn save_session(&self, session: SessionState) {
        if let Some(order) = self.barrier_order.lock().unwrap().as_ref() {
            order.lock().unwrap().push("persist");
        }
        assert!(
            !self
                .panic_on_save
                .load(std::sync::atomic::Ordering::Acquire),
            "injected store save panic"
        );
        *self.saved.lock().unwrap() = Some(session);
        self.events.lock().unwrap().push("save");
    }
    fn flush(&self) -> bool {
        if let Some(order) = self.barrier_order.lock().unwrap().as_ref() {
            order.lock().unwrap().push("store-preflight");
        }
        assert!(
            !self
                .panic_on_flush
                .load(std::sync::atomic::Ordering::Acquire),
            "injected store flush panic"
        );
        self.events.lock().unwrap().push("flush");
        self.flush_result.lock().unwrap().unwrap_or(true)
    }
    fn shutdown_until(&self, deadline: std::time::Instant) -> StoreShutdownOutcome {
        self.shutdown_entered
            .store(true, std::sync::atomic::Ordering::Release);
        self.shutdown_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if let Some(order) = self.barrier_order.lock().unwrap().as_ref() {
            order.lock().unwrap().push("store-final");
        }
        if let Some(order) = self.shutdown_order.lock().unwrap().as_ref() {
            order.lock().unwrap().push("store");
        }
        assert!(
            !self
                .panic_on_shutdown
                .load(std::sync::atomic::Ordering::Acquire),
            "injected store shutdown panic"
        );
        let late_blocker_update = self.blocker_update_on_shutdown.lock().unwrap().take();
        if let Some(outcome) = late_blocker_update {
            self.complete_blocker_update(outcome);
        }
        self.shutdown_outcome.lock().unwrap().unwrap_or_else(|| {
            if self.flush_until(deadline) {
                StoreShutdownOutcome::Clean
            } else {
                StoreShutdownOutcome::RetryableFailure
            }
        })
    }
    fn load_session(&self) -> SessionLoad {
        self.load_session_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        assert!(
            !self
                .panic_on_load
                .load(std::sync::atomic::Ordering::Acquire),
            "injected store panic"
        );
        if *self.load_failed.lock().unwrap() {
            return SessionLoad::Failed;
        }
        if let Some(reason) = self.recovery_reason.lock().unwrap().clone() {
            return SessionLoad::RecoveryRequired { reason };
        }
        let Some(state) = self.saved.lock().unwrap().clone() else {
            return SessionLoad::Absent;
        };
        let profiles = self.degraded_profiles.lock().unwrap().clone();
        let blocker_configs = self
            .blocker_configs
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| {
                state
                    .profiles
                    .iter()
                    .map(|profile| ProfileBlockerConfig {
                        profile: profile.id,
                        revision: BlockerConfigRevision::INITIAL,
                        config: BlockerConfig::default(),
                    })
                    .collect()
            });
        if profiles.is_empty() {
            SessionLoad::Loaded {
                state,
                blocker_configs,
            }
        } else {
            SessionLoad::LoadedWithDegradedProfiles {
                state,
                profiles,
                blocker_configs,
            }
        }
    }
    fn update_profile_blocker_config(
        &self,
        profile: ProfileId,
        expected: BlockerConfigRevision,
        next: BlockerConfig,
        done: Box<dyn FnOnce(BlockerConfigUpdateOutcome) + Send>,
    ) -> bool {
        if self
            .reject_blocker_updates
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return false;
        }
        if self
            .hold_blocker_updates
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.held_blocker_updates
                .lock()
                .unwrap()
                .push_back((profile, expected, next, done));
            return true;
        }
        let outcome = self
            .blocker_update_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                BlockerConfigUpdateOutcome::Updated(ProfileBlockerConfig {
                    profile,
                    revision: expected.next().expect("test revision must advance"),
                    config: next,
                })
            });
        done(outcome);
        true
    }
    fn load_profile_blocker_config(
        &self,
        profile: ProfileId,
        done: Box<dyn FnOnce(BlockerConfigLoadOutcome) + Send>,
    ) -> bool {
        self.blocker_load_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if self
            .shutdown_entered
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.blocker_load_after_shutdown_calls
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        if self
            .reject_blocker_loads
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return false;
        }
        if self
            .hold_blocker_loads
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.held_blocker_loads
                .lock()
                .unwrap()
                .push_back((profile, done));
            return true;
        }
        let outcome = self
            .blocker_load_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(BlockerConfigLoadOutcome::Failed);
        done(outcome);
        true
    }
    fn record_visit(&self, _profile: ProfileId, url: String, _title: String) {
        self.visits.lock().unwrap().push(url);
    }
    fn app_setting(&self, _key: &str) -> Option<String> {
        None
    }
    fn set_app_setting(&self, _key: String, _value: String) -> bool {
        !self
            .reject_settings
            .load(std::sync::atomic::Ordering::Acquire)
    }
    fn search_history(
        &self,
        _profile: ProfileId,
        _query: &str,
        _limit: u32,
    ) -> Vec<zephium_core::ports::store::HistoryHit> {
        self.history_started
            .store(true, std::sync::atomic::Ordering::Release);
        let delay = self
            .history_delay_ms
            .load(std::sync::atomic::Ordering::Acquire);
        if delay != 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
        self.history.clone()
    }
    fn favicon_age(&self, _profile: ProfileId, origin: &str) -> Option<i64> {
        self.icon_ages.lock().unwrap().get(origin).copied()
    }
    fn save_favicon(
        &self,
        _profile: ProfileId,
        origin: String,
        _content_type: Option<String>,
        bytes: Vec<u8>,
    ) {
        self.icons.lock().unwrap().push((origin, bytes));
    }
    fn favicon_bytes(
        &self,
        _profile: ProfileId,
        origin: &str,
    ) -> Option<(Option<String>, Vec<u8>)> {
        self.icons
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(stored_origin, _)| stored_origin == origin)
            .map(|(_, bytes)| {
                (
                    Some(zephium_core::icon::RGBA32_MIME.to_owned()),
                    bytes.clone(),
                )
            })
    }
    fn fresh_favicon_raster(
        &self,
        profile: ProfileId,
        origin: &str,
        max_age_seconds: i64,
    ) -> Option<Vec<u8>> {
        self.favicon_age(profile, origin)
            .is_some_and(|age| age <= max_age_seconds)
            .then(|| self.favicon_bytes(profile, origin))
            .flatten()
            .map(|(_, bytes)| bytes)
    }
    fn pending_profile_deletions(&self) -> ProfileDeletionLoad {
        self.pending_deletion_load_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if self
            .pending_load_failures
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |remaining| remaining.checked_sub(1),
            )
            .is_ok()
        {
            return ProfileDeletionLoad::Failed;
        }
        ProfileDeletionLoad::Loaded(self.pending_deletions.lock().unwrap().clone())
    }
    fn authorize_profile_deletion(
        &self,
        profile: ProfileId,
        filtered_session: SessionState,
        _deadline: std::time::Instant,
    ) -> ProfileDeletionAuthorizeOutcome {
        self.events.lock().unwrap().push("authorize-delete");
        self.authorized_sessions
            .lock()
            .unwrap()
            .push((profile, filtered_session.clone()));
        let outcome = self
            .authorize_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ProfileDeletionAuthorizeOutcome::NotRegistered);
        if matches!(
            outcome,
            ProfileDeletionAuthorizeOutcome::Authorized
                | ProfileDeletionAuthorizeOutcome::AlreadyAuthorized
        ) || (outcome == ProfileDeletionAuthorizeOutcome::OutcomeUnknown
            && self
                .authorize_unknown_commits
                .load(std::sync::atomic::Ordering::Acquire))
        {
            *self.saved.lock().unwrap() = Some(filtered_session);
            let mut pending = self.pending_deletions.lock().unwrap();
            if !pending.iter().any(|deletion| deletion.profile == profile) {
                pending.push(PendingProfileDeletion {
                    profile,
                    native_erasure_verified: false,
                    extension_native_namespace: *self.authorized_native_namespace.lock().unwrap(),
                });
            }
        }
        outcome
    }
    fn finalize_profile_deletion(
        &self,
        profile: ProfileId,
        _deadline: std::time::Instant,
    ) -> ProfileDeletionFinalizeOutcome {
        self.events.lock().unwrap().push("finalize-delete");
        let outcome = self
            .finalize_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ProfileDeletionFinalizeOutcome::NotAuthorized);
        if outcome == ProfileDeletionFinalizeOutcome::Completed
            || (outcome == ProfileDeletionFinalizeOutcome::OutcomeUnknown
                && self
                    .finalize_unknown_completes
                    .load(std::sync::atomic::Ordering::Acquire))
        {
            self.pending_deletions
                .lock()
                .unwrap()
                .retain(|deletion| deletion.profile != profile);
        } else if let Some(deletion) = self
            .pending_deletions
            .lock()
            .unwrap()
            .iter_mut()
            .find(|deletion| deletion.profile == profile)
        {
            // A finalize attempt durably records native proof before its
            // local purge can fail.
            deletion.native_erasure_verified = true;
        }
        outcome
    }
}

pub(crate) struct FakeChrome;
impl GeometryChrome for FakeChrome {
    fn position(&self, _frame: ChromeFrame) -> bool {
        true
    }
}
impl PresentationChrome for FakeChrome {
    fn apply_tab_for_presentation(
        &self,
        _presentation: ChromePresentation,
        _done: ChromePresentationCallback,
    ) -> ChromePresentationDispatch {
        ChromePresentationDispatch::Applied
    }
}

#[derive(Default)]
struct AsyncChrome {
    pending: Mutex<VecDeque<(ChromePresentation, ChromePresentationCallback)>>,
    reject_admission: std::sync::atomic::AtomicBool,
}

impl GeometryChrome for AsyncChrome {
    fn position(&self, _frame: ChromeFrame) -> bool {
        true
    }
}

impl PresentationChrome for AsyncChrome {
    fn apply_tab_for_presentation(
        &self,
        presentation: ChromePresentation,
        done: ChromePresentationCallback,
    ) -> ChromePresentationDispatch {
        if self
            .reject_admission
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return ChromePresentationDispatch::Rejected;
        }
        self.pending.lock().unwrap().push_back((presentation, done));
        ChromePresentationDispatch::Scheduled
    }
}

impl AsyncChrome {
    fn presentations(&self) -> Vec<ChromePresentation> {
        self.pending
            .lock()
            .unwrap()
            .iter()
            .map(|(presentation, _)| presentation.clone())
            .collect()
    }

    fn complete_next(&self, applied: bool) -> ChromePresentation {
        let (presentation, done) = self
            .pending
            .lock()
            .unwrap()
            .pop_front()
            .expect("an exact privileged presentation must be pending");
        done(applied);
        presentation
    }
}

// Materializes projections the way the frontend store does: snapshots
// replace, deltas patch one row.
type Screen = Arc<Mutex<ItemsState>>;

fn apply_projection(view: &mut ItemsState, p: Projection) {
    match p {
        Projection::Items(s) => *view = s,
        Projection::Tab(t) => {
            if let Some(slot) = view.tabs.iter_mut().find(|x| x.id == t.id) {
                *slot = t;
            }
        }
        Projection::UiCommand(_) => {}
        Projection::Search(_) => {}
        Projection::Layout(_) => {}
        Projection::RuntimeStatus(_) => {}
        Projection::BlockerStatus(_) => {}
        Projection::OperationProcessed(_) => {}
    }
}

fn setup_with(store: Arc<FakeStore>) -> (Shell, Arc<FakeEngine>, Screen) {
    setup_with_extension_lifecycle(store, clean_extension_lifecycle())
}

fn setup_with_extension_lifecycle(
    store: Arc<FakeStore>,
    extension_service: ExtensionLifecycle,
) -> (Shell, Arc<FakeEngine>, Screen) {
    let engine = Arc::new(FakeEngine::default());
    let screen: Screen = Arc::new(Mutex::new(ItemsState {
        projection_revision: String::new(),
        profile: None,
        spaces: Vec::new(),
        active_space_id: None,
        nodes: Vec::new(),
        tabs: Vec::new(),
        active: None,
        split_group: None,
    }));
    let sink = screen.clone();
    let mut shell = Shell::new_with_extension_lifecycle(
        engine.clone(),
        store,
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Arc::new(FakeChrome),
        Box::new(move |p| apply_projection(&mut sink.lock().unwrap(), p)),
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    (shell, engine, screen)
}

fn setup() -> (Shell, Arc<FakeEngine>, Screen) {
    setup_with(Arc::new(FakeStore::default()))
}

fn setup_with_async_chrome() -> (Shell, Arc<FakeEngine>, Arc<AsyncChrome>, Screen) {
    let engine = Arc::new(FakeEngine::default());
    let chrome = Arc::new(AsyncChrome::default());
    let screen: Screen = Arc::new(Mutex::new(ItemsState {
        projection_revision: String::new(),
        profile: None,
        spaces: Vec::new(),
        active_space_id: None,
        nodes: Vec::new(),
        tabs: Vec::new(),
        active: None,
        split_group: None,
    }));
    let sink = screen.clone();
    let mut shell = Shell::new(
        engine.clone(),
        Arc::new(FakeStore::default()),
        chrome.clone(),
        Box::new(move |projection| apply_projection(&mut sink.lock().unwrap(), projection)),
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    (shell, engine, chrome, screen)
}

type OperationLog = Arc<Mutex<Vec<OperationDisposition>>>;

fn setup_with_operation_log(
    store: Arc<FakeStore>,
) -> (Shell, Arc<FakeEngine>, Screen, OperationLog) {
    setup_with_operation_log_and_lifecycle(store, clean_extension_lifecycle(), Box::new(|_| {}))
}

fn setup_with_operation_log_and_lifecycle(
    store: Arc<FakeStore>,
    extension_service: ExtensionLifecycle,
    terminal_failure: ShellTerminalFailureCallback,
) -> (Shell, Arc<FakeEngine>, Screen, OperationLog) {
    let engine = Arc::new(FakeEngine::default());
    let screen: Screen = Arc::new(Mutex::new(ItemsState {
        projection_revision: String::new(),
        profile: None,
        spaces: Vec::new(),
        active_space_id: None,
        nodes: Vec::new(),
        tabs: Vec::new(),
        active: None,
        split_group: None,
    }));
    let operations: OperationLog = Arc::new(Mutex::new(Vec::new()));
    let sink = screen.clone();
    let operation_sink = operations.clone();
    let mut shell = Shell::new_with_extension_lifecycle_and_failure(
        engine.clone(),
        store,
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        terminal_failure,
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            if let Projection::OperationProcessed(completion) = &projection {
                operation_sink.lock().unwrap().push(completion.clone());
            }
            apply_projection(&mut sink.lock().unwrap(), projection);
        }),
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    (shell, engine, screen, operations)
}

fn add_inactive_named_profile(shell: &mut Shell, seed: u128) -> ProfileId {
    let profile = ProfileId::from(seed);
    let space = SpaceId::from(seed + 1);
    assert!(shell.profiles.insert(Profile {
        id: profile,
        name: "Deletable".into(),
        kind: ProfileKind::Named,
    }));
    assert!(shell.initialize_new_blocker_profile(profile));
    assert!(shell.spaces.insert(Space {
        id: space,
        profile,
        name: "Deletable".into(),
    }));
    profile
}

fn delete_operation(operation_id: &str, profile: ProfileId) -> Command {
    Command::Operation {
        operation_id: operation_id.into(),
        command: Box::new(Command::DeleteProfile(profile)),
    }
}

fn test_shutdown_deadline() -> std::time::Instant {
    std::time::Instant::now() + END_TO_END_SHUTDOWN_TIMEOUT
}

fn last(screen: &Screen) -> ItemsState {
    screen.lock().unwrap().clone()
}

fn active_id(screen: &Screen) -> ItemId {
    ItemId::parse(&last(screen).active.unwrap()).unwrap()
}

fn persisted_zoom(store: &FakeStore, id: ItemId) -> f64 {
    let saved = store.saved.lock().unwrap().clone().unwrap();
    let item = saved.items.iter().find(|item| item.id == id).unwrap();
    let PersistedKind::Tab { zoom, .. } = &item.kind else {
        panic!("expected persisted tab")
    };
    *zoom
}

fn navigate_and_commit(shell: &mut Shell, id: ItemId, input: &str) {
    let url = navigation::classify(input)
        .expect("test navigation must be valid")
        .to_string();
    shell.handle(Command::Navigate {
        id,
        input: input.into(),
    });
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: url.clone(),
    }));
    present_committed(shell, id, &url);
}

fn present_committed(shell: &mut Shell, id: ItemId, url: &str) {
    static NEXT_PRESENTATION: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(10_000);
    let navigation = NEXT_PRESENTATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    shell.handle(Command::Engine(presentation_pending(
        id,
        NavigationPresentationId::from_raw(navigation),
        url,
    )));
}

fn commit_url(shell: &mut Shell, id: ItemId, url: &str) {
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: url.into(),
    }));
}

fn presentation_pending(
    id: ItemId,
    navigation: NavigationPresentationId,
    url: &str,
) -> EngineEvent {
    EngineEvent::PresentationPending {
        id,
        navigation,
        url: url.into(),
    }
}

fn presentation_ready(id: ItemId, navigation: NavigationPresentationId, url: &str) -> EngineEvent {
    EngineEvent::PresentationReady {
        id,
        navigation,
        url: url.into(),
    }
}

fn first_probing_discard(shell: &Shell) -> (ItemId, DiscardProbeId) {
    shell
        .residency
        .discard_probes
        .iter()
        .find_map(|(id, state)| match state {
            PendingDiscardProbe::Probing { probe, .. } => Some((*id, *probe)),
            PendingDiscardProbe::Closing { .. } => None,
        })
        .expect("a discard probe must be in flight")
}

fn acknowledge_safe_discard(shell: &mut Shell, id: ItemId, probe: DiscardProbeId) {
    let profile = shell.profile_of_item(id).unwrap();
    shell.handle(Command::Engine(EngineEvent::DiscardSafety {
        id,
        probe,
        can_discard: true,
    }));
    assert!(matches!(
        shell.residency.discard_probes.get(&id),
        Some(PendingDiscardProbe::Closing { probe: pending, .. }) if *pending == probe
    ));
    shell.handle(Command::Engine(EngineEvent::ViewDiscarded {
        id,
        profile,
        probe,
    }));
}

mod actor_integration;
mod blocker;
mod bootstrap;
mod engine_events;
mod favicons;
#[path = "navigation.rs"]
mod navigation_tests;
mod operations;
mod persistence;
mod presentation;
mod profile_deletion;
mod projections;
mod search;
mod shutdown;
mod tabs;
mod view_lifecycle;
mod window_layout;
mod zoom;
