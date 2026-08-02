use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::blocker::{ContentPolicyGeneration, ContentRuleApplyFailure, ContentRules};
use crate::geometry::Rect;
use crate::ids::{ItemId, ProfileId, ScriptId, ScriptPrincipalId, WindowId};
use crate::injection::MatchSet;
pub use crate::permissions::PagePermissionKind as PermissionKind;
use crate::runtime_security::RuntimeSecurityAdvisories;
use crate::split::Pane;

/// Which engine data partition a view lives in. Every persistent profile gets
/// its own engine store; incognito is ephemeral and never intentionally
/// persists browsing data. Within one engine process a `ProfileId` is
/// permanently bound to either the durable class (`Default`/`Persistent`) or
/// `Ephemeral`; callers must mint a new id instead of changing that class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Partition {
    Default(ProfileId),
    Persistent(ProfileId),
    Ephemeral(ProfileId),
}

impl Partition {
    pub fn profile(self) -> ProfileId {
        match self {
            Partition::Default(p) | Partition::Persistent(p) | Partition::Ephemeral(p) => p,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContentScope {
    Global,
    Profile(ProfileId),
}

/// Native security principal for one isolated script world.
///
/// The variant is part of the identity: a userscript and extension can never
/// alias merely because their persistent ids happen to contain equal bytes.
/// Native adapters derive world and handler names from this value and must
/// never accept a principal supplied by page JavaScript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScriptPrincipal {
    Userscript(ScriptPrincipalId),
    Extension(ScriptPrincipalId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptOwner {
    Builtin,
    Principal(ScriptPrincipal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum World {
    Page,
    Isolated(ScriptPrincipal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
    DocumentIdle,
}

/// Exact process-local identity for one desired user-content generation.
/// Values never wrap: exhausted owners must restart instead of risking a late
/// native callback being accepted as a newer replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserContentGeneration(u64);

impl UserContentGeneration {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// What the engine can truthfully establish at its synchronous call boundary.
///
/// `Scheduled` is deliberately not a claim that WebKit/WebView2 applied the
/// action, navigated, or produced a renderer result. It proves that the exact,
/// lifecycle-checked task was admitted to the owning native UI event loop. A
/// task can still be invalidated by an ordered close before that queue runs.
///
/// Some platforms have a second bounded host queue because a native engine
/// can pump the event loop re-entrantly. Refusal at that later boundary is not
/// silently treated as success: the backend terminally revokes content
/// authority and invokes its mandatory fatal callback.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeDispatch {
    Scheduled,
    Rejected,
    Unsupported,
}

/// Opaque identity for one native main-frame navigation presentation.
///
/// The browser shell may return this token only to [`Engine::present_navigation`].
/// URLs are deliberately not identities: redirects and overlapping loads can
/// otherwise reveal content before chrome has attributed the exact commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NavigationPresentationId(u64);

impl NavigationPresentationId {
    /// Native adapters mint non-wrapping identities from their own exact
    /// navigation epochs. Callers must treat the value as opaque.
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub const fn into_raw(self) -> u64 {
        self.0
    }
}

impl NativeDispatch {
    pub fn from_scheduled(scheduled: bool) -> Self {
        if scheduled {
            Self::Scheduled
        } else {
            Self::Rejected
        }
    }
}

/// Result of permanently retiring an engine profile and erasing the native
/// web engine data it owned.
///
/// `Verified` is deliberately narrow: the engine has released every native
/// view/context it knows for the profile and proved its engine-owned data
/// store absent (or empty for an ephemeral native store). It does not include
/// Zephium's SQLite/session data; callers must coordinate that separate store
/// transaction before removing a profile from the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileDataErasureOutcome {
    /// Native work settled and the scoped engine data was proven absent.
    Verified,
    /// The native attempt reached a terminal failure, including refusal to
    /// dispatch it. A retry may be admitted, but the synchronous retirement
    /// gate remains permanent and this outcome is never proof that existing
    /// native references closed or that their data was deleted. A dispatcher
    /// refusal is terminal for continued browsing and requires process exit
    /// unless a later attempt reaches `Verified`.
    Failed,
    /// The caller's bounded wait elapsed. This is not native cancellation or
    /// settlement; the in-process attempt remains active until a later native
    /// terminal callback, so an immediate retry must be rejected. Existing
    /// native references and data are unproven; continued browsing is unsafe
    /// and the coordinator must drive verified teardown or process exit.
    TimedOut,
}

/// One pipeline for everything injected into content: cosmetic CSS, Boosts,
/// userscripts, adblock cosmetics and a future extensions layer.
pub const MAX_USER_SCRIPTS_PER_SCOPE: usize = 256;
pub const MAX_USER_STYLES_PER_SCOPE: usize = 256;
pub const MAX_USER_SCRIPTS_PER_OWNER: usize = 64;
pub const MAX_USER_STYLES_PER_OWNER: usize = 64;
pub const MAX_USER_SCRIPT_BYTES: usize = 2 * 1024 * 1024;
// JSON escaping can expand one CSS byte to six source bytes before the
// document-start wrapper is installed. Keep the worst-case generated script
// below MAX_USER_SCRIPT_BYTES without needing an unbounded second pass.
pub const MAX_USER_STYLE_BYTES: usize = 256 * 1024;
pub const MAX_USER_CONTENT_BYTES_PER_SCOPE: usize = 16 * 1024 * 1024;
pub const MAX_USER_CONTENT_BYTES_PER_OWNER: usize = 4 * 1024 * 1024;
pub const MAX_USER_CONTENT_RETAINED_BYTES_PER_OWNER: usize = 16 * 1024 * 1024;
pub const MAX_USER_CONTENT_RETAINED_BYTES_PER_SCOPE: usize = 32 * 1024 * 1024;
pub const MAX_USER_CONTENT_RETAINED_BYTES_PROCESS: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserContent {
    pub scripts: Vec<UserScript>,
    pub styles: Vec<UserStyle>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserScript {
    pub id: ScriptId,
    pub owner: ScriptOwner,
    pub source: Arc<str>,
    pub world: World,
    pub matches: MatchSet,
    pub run_at: RunAt,
    pub all_frames: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserStyle {
    pub id: ScriptId,
    pub owner: ScriptOwner,
    pub css: Arc<str>,
    pub matches: MatchSet,
    pub all_frames: bool,
}

/// Stable registration identity. Script ids are owner-local, so native diff
/// maps must retain both fields when global/profile sets are composed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UserScriptKey {
    pub owner: ScriptOwner,
    pub id: ScriptId,
}

impl UserScript {
    pub const fn key(&self) -> UserScriptKey {
        UserScriptKey {
            owner: self.owner,
            id: self.id,
        }
    }
}

impl UserStyle {
    pub const fn key(&self) -> UserScriptKey {
        UserScriptKey {
            owner: self.owner,
            id: self.id,
        }
    }
}

fn user_script_retained_budget_bytes(script: &UserScript) -> Option<usize> {
    script
        .source
        .len()
        .checked_add(script.matches.retained_budget_bytes())?
        .checked_add(256)
}

fn user_style_retained_budget_bytes(style: &UserStyle) -> Option<usize> {
    // JSON string escaping is at most six ASCII bytes per UTF-8 input byte
    // for the control characters that expand the most. Charge the cached
    // wrapper before it is materialized in the host.
    style
        .css
        .len()
        .checked_mul(6)?
        .checked_add(512)?
        .checked_add(style.matches.retained_budget_bytes())?
        .checked_add(256)
}

impl UserContent {
    /// Conservative process-memory charge used by both synchronous dispatch
    /// admission and the host's retained-registry budget.
    pub fn retained_budget_bytes(&self) -> Option<usize> {
        self.scripts
            .iter()
            .map(user_script_retained_budget_bytes)
            .chain(self.styles.iter().map(user_style_retained_budget_bytes))
            .try_fold(0_usize, |total, bytes| total.checked_add(bytes?))
    }

    /// Validates allocation and principal invariants before content reaches a
    /// native adapter. The returned refusal list is bounded by the already
    /// checked script/style count limits.
    pub fn validate(&self) -> Result<(), UserContentApplyFailure> {
        if self.scripts.len() > MAX_USER_SCRIPTS_PER_SCOPE {
            return Err(UserContentApplyFailure::TooManyScripts);
        }
        if self.styles.len() > MAX_USER_STYLES_PER_SCOPE {
            return Err(UserContentApplyFailure::TooManyStyles);
        }

        #[derive(Default)]
        struct OwnerUsage {
            scripts: usize,
            styles: usize,
            source_bytes: usize,
            retained_bytes: usize,
        }

        let mut total_bytes = 0_usize;
        let mut retained_bytes = 0_usize;
        let mut owner_usage = HashMap::<ScriptOwner, OwnerUsage>::new();
        let mut ids = HashSet::with_capacity(self.scripts.len() + self.styles.len());
        let mut refusals = Vec::new();
        for script in &self.scripts {
            total_bytes = total_bytes
                .checked_add(script.source.len())
                .ok_or(UserContentApplyFailure::TotalSourceTooLarge)?;
            let registration_bytes = user_script_retained_budget_bytes(script)
                .ok_or(UserContentApplyFailure::TotalRetainedTooLarge)?;
            retained_bytes = retained_bytes
                .checked_add(registration_bytes)
                .ok_or(UserContentApplyFailure::TotalRetainedTooLarge)?;
            let usage = owner_usage.entry(script.owner).or_default();
            usage.scripts = usage.scripts.saturating_add(1);
            usage.source_bytes = usage
                .source_bytes
                .checked_add(script.source.len())
                .ok_or(UserContentApplyFailure::OwnerBudgetExceeded)?;
            usage.retained_bytes = usage
                .retained_bytes
                .checked_add(registration_bytes)
                .ok_or(UserContentApplyFailure::OwnerBudgetExceeded)?;
            if script.source.is_empty() {
                refusals.push(UserScriptRefusal {
                    registration: script.key(),
                    reason: UserScriptRefusalReason::EmptySource,
                });
            } else if script.source.len() > MAX_USER_SCRIPT_BYTES {
                refusals.push(UserScriptRefusal {
                    registration: script.key(),
                    reason: UserScriptRefusalReason::SourceTooLarge,
                });
            } else if script.source.as_bytes().contains(&0) {
                // WebKitGTK consumes NUL-terminated UTF-8 and would otherwise
                // install only the caller-controlled prefix. Keep the core
                // contract identical on every platform instead of relying on
                // an adapter-specific conversion failure.
                refusals.push(UserScriptRefusal {
                    registration: script.key(),
                    reason: UserScriptRefusalReason::EmbeddedNul,
                });
            }
            let owner_matches_world = matches!(
                (script.owner, script.world),
                (ScriptOwner::Builtin, World::Page)
            ) || matches!(
                (script.owner, script.world),
                (ScriptOwner::Principal(owner), World::Isolated(world)) if owner == world
            );
            if !owner_matches_world {
                refusals.push(UserScriptRefusal {
                    registration: script.key(),
                    reason: UserScriptRefusalReason::OwnerWorldMismatch,
                });
            }
            if !ids.insert(script.key()) {
                refusals.push(UserScriptRefusal {
                    registration: script.key(),
                    reason: UserScriptRefusalReason::DuplicateId,
                });
            }
        }
        for style in &self.styles {
            total_bytes = total_bytes
                .checked_add(style.css.len())
                .ok_or(UserContentApplyFailure::TotalSourceTooLarge)?;
            let registration_bytes = user_style_retained_budget_bytes(style)
                .ok_or(UserContentApplyFailure::TotalRetainedTooLarge)?;
            retained_bytes = retained_bytes
                .checked_add(registration_bytes)
                .ok_or(UserContentApplyFailure::TotalRetainedTooLarge)?;
            let usage = owner_usage.entry(style.owner).or_default();
            usage.styles = usage.styles.saturating_add(1);
            usage.source_bytes = usage
                .source_bytes
                .checked_add(style.css.len())
                .ok_or(UserContentApplyFailure::OwnerBudgetExceeded)?;
            usage.retained_bytes = usage
                .retained_bytes
                .checked_add(registration_bytes)
                .ok_or(UserContentApplyFailure::OwnerBudgetExceeded)?;
            if style.css.is_empty() {
                refusals.push(UserScriptRefusal {
                    registration: style.key(),
                    reason: UserScriptRefusalReason::EmptySource,
                });
            } else if style.css.len() > MAX_USER_STYLE_BYTES {
                refusals.push(UserScriptRefusal {
                    registration: style.key(),
                    reason: UserScriptRefusalReason::SourceTooLarge,
                });
            } else if style.css.as_bytes().contains(&0) {
                refusals.push(UserScriptRefusal {
                    registration: style.key(),
                    reason: UserScriptRefusalReason::EmbeddedNul,
                });
            }
            if !ids.insert(style.key()) {
                refusals.push(UserScriptRefusal {
                    registration: style.key(),
                    reason: UserScriptRefusalReason::DuplicateId,
                });
            }
        }
        if total_bytes > MAX_USER_CONTENT_BYTES_PER_SCOPE {
            return Err(UserContentApplyFailure::TotalSourceTooLarge);
        }
        if retained_bytes > MAX_USER_CONTENT_RETAINED_BYTES_PER_SCOPE {
            return Err(UserContentApplyFailure::TotalRetainedTooLarge);
        }
        if owner_usage.values().any(|usage| {
            usage.scripts > MAX_USER_SCRIPTS_PER_OWNER
                || usage.styles > MAX_USER_STYLES_PER_OWNER
                || usage.source_bytes > MAX_USER_CONTENT_BYTES_PER_OWNER
                || usage.retained_bytes > MAX_USER_CONTENT_RETAINED_BYTES_PER_OWNER
        }) {
            return Err(UserContentApplyFailure::OwnerBudgetExceeded);
        }
        if refusals.is_empty() {
            Ok(())
        } else {
            Err(UserContentApplyFailure::Scripts(refusals))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UserScriptRefusalReason {
    EmptySource,
    SourceTooLarge,
    EmbeddedNul,
    OwnerWorldMismatch,
    DuplicateId,
    UnsupportedWorld,
    UnsupportedRunAt,
    UnsupportedFrameTarget,
    UnsupportedMatchSet,
    HostOnlyOwner,
    InvalidScope,
    ProtectedRegistration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UserScriptRefusal {
    pub registration: UserScriptKey,
    pub reason: UserScriptRefusalReason,
}

/// Stable, bounded failure classes for one atomic user-content replacement.
/// Native/parser strings and injected source are intentionally absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserContentApplyFailure {
    StaleGeneration,
    TooManyScripts,
    TooManyStyles,
    TooManyScopes,
    ReservedScope,
    TotalSourceTooLarge,
    TotalRetainedTooLarge,
    OwnerBudgetExceeded,
    ProcessBudgetExceeded,
    Scripts(Vec<UserScriptRefusal>),
    NativeInstallation,
    NativeCleanup,
    UnsupportedPlatform,
}

/// Terminal native state for one desired user-content generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserContentSettlement {
    Applied {
        generation: UserContentGeneration,
    },
    Retained {
        generation: UserContentGeneration,
        failure: UserContentApplyFailure,
    },
    Unavailable {
        failure: UserContentApplyFailure,
    },
}

/// A resolved keyboard shortcut for platforms where the engine must
/// intercept keys natively (WebView2 AcceleratorKeyPressed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub id: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: u32,
}

pub trait Engine {
    /// Schedules creation on the native UI thread. `false` means the request
    /// was not admitted at all, so the shell must roll back its live-view bit.
    fn create_view(&self, id: ItemId, partition: Partition, url: &str, bounds: Rect) -> bool;
    /// Schedules a navigation on the native UI thread. `false` means the
    /// request was not admitted. An admitted request can still fail before
    /// its identity-bearing native commit, synchronously or asynchronously;
    /// that arrives as `EngineEvent::NavigationFailed` with the same request
    /// id.
    fn navigate(&self, id: ItemId, url: &str, request: NavigationRequestId) -> bool;
    /// Reveals an initially hidden native view only if `navigation` is still
    /// the exact committed main-frame epoch whose URL was delivered to the
    /// shell. The shell calls this only after privileged chrome has applied
    /// and verified the matching revision-bearing URL projection; native
    /// completion may re-drive the same idempotent acknowledgement.
    fn present_navigation(
        &self,
        _id: ItemId,
        _navigation: NavigationPresentationId,
    ) -> NativeDispatch {
        NativeDispatch::Unsupported
    }
    fn reload(&self, id: ItemId) -> NativeDispatch;
    fn stop(&self, id: ItemId) -> NativeDispatch;
    fn go_back(&self, id: ItemId) -> NativeDispatch;
    fn go_forward(&self, id: ItemId) -> NativeDispatch;
    fn close(&self, id: ItemId) -> NativeDispatch;
    /// Lay the split `tree` into `region` of `window`, or hide that window's
    /// content when `region` is `None`. The engine owns pane geometry so
    /// resize stays in the native pass.
    fn set_content(
        &self,
        window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) -> NativeDispatch;
    fn set_drop_indicator(&self, window: WindowId, zone: Option<Rect>) -> NativeDispatch;
    /// Requests an exact page zoom for the current native-view generation.
    /// Queue admission is not application: the authoritative native scale
    /// arrives as [`EngineEvent::ZoomSettled`] carrying the same `request`.
    fn zoom(&self, id: ItemId, scale: f64, request: ZoomRequestId) -> NativeDispatch;
    fn set_muted(&self, id: ItemId, muted: bool) -> NativeDispatch;
    /// `None` clears the current find session.
    fn find(&self, id: ItemId, query: Option<&str>) -> NativeDispatch;
    /// Result arrives as `EngineEvent::Captured`.
    fn capture(&self, id: ItemId) -> NativeDispatch;
    /// Result arrives as `EngineEvent::HtmlExtracted`.
    fn extract_html(&self, id: ItemId) -> NativeDispatch;
    /// Asks the sandboxed page renderer to fetch and decode its best icon;
    /// an exact 32x32 RGBA result arrives as `EngineEvent::FaviconPixels`.
    /// Repeated calls poll the renderer-owned asynchronous decode state.
    fn discover_favicon(&self, id: ItemId) -> NativeDispatch;
    /// Asynchronously asks the exact live native-view generation and its
    /// current committed navigation whether it is safe to discard. A missing
    /// or malformed response is deliberately not a positive result. Callers
    /// must correlate `probe` with `EngineEvent::DiscardSafety` and recheck
    /// visibility/loading state before closing the view.
    fn probe_discard_safety(&self, id: ItemId, probe: DiscardProbeId) -> bool;
    /// Retires the exact live view after a positive probe. Completion arrives
    /// only after native destruction and the same-id lifecycle gate are both
    /// settled, allowing lazy recreation without racing an asynchronous close.
    fn discard_view(&self, id: ItemId, probe: DiscardProbeId) -> bool;
    fn print(&self, id: ItemId) -> NativeDispatch;
    /// Atomically replaces one ownership scope's desired injected content.
    /// Queue admission is not native application; the terminal outcome is
    /// reported as [`EngineEvent::UserContentSettled`]. `Rejected` is a
    /// synchronous terminal result for malformed/over-budget candidates,
    /// reserved host scope, lifecycle retirement, or bounded in-flight
    /// backpressure; no settlement follows a rejected dispatch.
    fn set_user_content(
        &self,
        scope: ContentScope,
        generation: UserContentGeneration,
        content: UserContent,
    ) -> NativeDispatch;
    fn set_shortcuts(&self, shortcuts: Vec<Shortcut>);
    /// Prebuild a hidden webview for `partition` so the next open adopts it
    /// instead of paying the renderer spawn. Safe moment: after a page load.
    fn warm_spare(&self, _partition: Partition) {}
    /// Hidden views the shell's idle policy wants suspended. The engine
    /// suspends where it has a primitive (WebView2) and resumes implicitly
    /// when a view becomes visible again.
    fn set_dormant(&self, _ids: Vec<ItemId>) {}
    /// Installs one exact, immutable profile-scoped content policy.
    ///
    /// Queue admission is not native application. The terminal result arrives
    /// as [`EngineEvent::ContentRulesSettled`]. A profile has no implicit
    /// allow-all state: callers must install an explicit `AllowAll` generation
    /// before its first view can be created.
    fn install_content_rules(
        &self,
        _profile: ProfileId,
        _generation: ContentPolicyGeneration,
        _rules: Arc<ContentRules>,
    ) -> NativeDispatch {
        NativeDispatch::Unsupported
    }
    /// Sticky process-local signal that the native browser runtime reported a
    /// newer version. `true` never means the running environments adopted the
    /// update: the composition root must use its ordinary ordered shutdown
    /// path and perform a whole-application restart, including privileged
    /// chrome, before clearing this state.
    fn runtime_restart_required(&self) -> bool {
        false
    }
    /// Non-fatal result of the process-start native runtime assessment.
    ///
    /// This is immutable for the current native process generation. It is
    /// computed locally before WebView construction and performs no network,
    /// filesystem, actor, or page-derived work.
    fn runtime_security_advisories(&self) -> RuntimeSecurityAdvisories {
        RuntimeSecurityAdvisories::new()
    }
    /// Permanently tombstones `profile` at the synchronous call boundary,
    /// rejects all future native access to it except cleanup, and schedules
    /// closure of every existing view/context plus asynchronous erasure of
    /// the profile's engine-owned data. The completion is invoked exactly
    /// once.
    ///
    /// Retirement does not depend on native-thread dispatch: even `Failed`
    /// leaves the profile inaccessible for the rest of this engine process.
    /// When dispatch itself fails, the backend must also reject continued
    /// content use globally because existing native pages cannot be proven
    /// closed; callers must terminate unless a retry verifies erasure.
    /// `Failed` may otherwise be retried because the native attempt is
    /// terminal, but it must never be interpreted as successful teardown.
    /// `TimedOut` is only a one-shot report to the caller: retry remains denied
    /// while the old native work might still be running, and a late terminal
    /// callback releases admission without invoking `done` again.
    fn erase_profile_data(
        &self,
        _profile: ProfileId,
        done: Box<dyn FnOnce(ProfileDataErasureOutcome) + Send>,
    );
    /// Close every native view/context on its owning thread. Completion runs
    /// only after native references have been released; composition roots use
    /// this as the final process-lifecycle barrier.
    fn shutdown(&self, done: Box<dyn FnOnce(bool) + Send>) {
        done(true);
    }
}

/// Correlates an explicit shell navigation with a native failure before its
/// identity-bearing main-frame commit.
///
/// Successful commits intentionally do not carry this token: page-initiated
/// navigations, redirects and same-document history changes have no shell
/// request and all use the same authoritative `UrlChanged` path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NavigationRequestId(pub u64);

/// Process-local correlation identity for one native page-zoom request.
///
/// Zoom is persisted only after the exact live native generation reports its
/// applied scale. Keeping this distinct from navigation identity prevents a
/// late result from a replaced view from mutating the replacement tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ZoomRequestId(pub u64);

/// Process-local correlation identity for one renderer-state discard probe.
/// It is never persisted or accepted from page content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiscardProbeId(pub u64);

/// A native browser action whose synchronous platform invocation failed.
///
/// This does not describe page-load completion. Reload/history success still
/// settles through the ordinary navigation callbacks; this enum exists so an
/// HRESULT/native refusal is never silently discarded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NativeAction {
    Reload,
    GoBack,
    GoForward,
}

/// Terminal native state for one requested content-policy generation.
///
/// The shape deliberately cannot express contradictory states such as a
/// successful application with a failure, or a failed replacement without
/// saying whether a prior generation remains active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentRuleSettlement {
    Applied {
        generation: ContentPolicyGeneration,
    },
    Retained {
        generation: ContentPolicyGeneration,
        failure: ContentRuleApplyFailure,
    },
    Unavailable {
        failure: ContentRuleApplyFailure,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// A native browser environment reported that a newer runtime is
    /// available. This is a process-global, sticky notification: recycling a
    /// content profile alone cannot update privileged chrome and must not be
    /// presented as successful adoption.
    RuntimeRestartRequired,
    /// One exact native content-policy installation attempt settled.
    ///
    /// Replacement failure reports `Retained` with the prior known-good
    /// generation. `Unavailable` means no explicit policy is active, so the
    /// engine continues to reject view creation for this profile.
    ///
    /// On WebKit, native rule-list changes govern future resource loads and
    /// navigations; settlement is not a claim that resources already loaded
    /// by the current document were retroactively filtered.
    ContentRulesSettled {
        profile: ProfileId,
        requested: ContentPolicyGeneration,
        settlement: ContentRuleSettlement,
    },
    /// One exact user-content replacement settled. A retained result means
    /// the prior generation remains the only authoritative native set. The
    /// shell's bounded mailbox may retain only the newest settlement per
    /// scope, so owners reconcile by monotonic `requested` generation rather
    /// than waiting independently on every superseded intermediate event.
    UserContentSettled {
        scope: ContentScope,
        requested: UserContentGeneration,
        settlement: UserContentSettlement,
    },
    TitleChanged {
        id: ItemId,
        title: String,
    },
    UrlChanged {
        id: ItemId,
        url: String,
    },
    /// The exact committed URL has already been delivered ahead of this
    /// event. After applying that URL to privileged chrome, the shell returns
    /// this opaque identity immediately to authorize the first presentation.
    PresentationPending {
        id: ItemId,
        navigation: NavigationPresentationId,
        /// Exact canonical URL emitted through `UrlChanged` immediately
        /// before this token. The shell must match both facts before it can
        /// acknowledge presentation; callback/queue order alone is not an
        /// authorization boundary.
        url: String,
    },
    /// Native completion re-drives the same exact presentation fact. This is
    /// an idempotent recovery path when a bounded queue coalesced `Pending`;
    /// it is not a prerequisite or an intentional first-paint delay.
    PresentationReady {
        id: ItemId,
        navigation: NavigationPresentationId,
        /// Same committed URL bound to `navigation`. This is repeated because
        /// a bounded lifecycle queue may coalesce `Pending` into `Ready`.
        url: String,
    },
    NavigationFailed {
        id: ItemId,
        request: NavigationRequestId,
    },
    /// The exact native zoom invocation settled. `applied_scale` is the
    /// adapter's last successfully applied scale for this view generation, so
    /// the newest event remains authoritative even when intermediate results
    /// are coalesced under pressure.
    ZoomSettled {
        id: ItemId,
        request: ZoomRequestId,
        applied_scale: f64,
        succeeded: bool,
    },
    /// A reload/history platform call returned an error. An `Ok` call is only
    /// native invocation success, never a claim that navigation completed.
    NativeActionFailed {
        id: ItemId,
        action: NativeAction,
    },
    LoadingChanged {
        id: ItemId,
        loading: bool,
    },
    FaviconPixels {
        id: ItemId,
        page_url: String,
        rgba: Vec<u8>,
    },
    /// Positive results have already passed exact native-view generation,
    /// navigation-epoch, fixed-schema DOM-state, and (where available)
    /// native audio-state checks. The shell still owns the final visibility
    /// and idle/budget revalidation.
    DiscardSafety {
        id: ItemId,
        probe: DiscardProbeId,
        can_discard: bool,
    },
    ViewDiscarded {
        id: ItemId,
        profile: ProfileId,
        probe: DiscardProbeId,
    },
    NavState {
        id: ItemId,
        can_go_back: bool,
        can_go_forward: bool,
    },
    NewWindowRequested {
        id: ItemId,
        url: String,
    },
    PermissionRequested {
        id: ItemId,
        origin: String,
        kind: PermissionKind,
    },
    DownloadRequested {
        id: ItemId,
        url: String,
    },
    ViewCreationFailed {
        id: ItemId,
    },
    /// The engine has already revoked and physically removed every listed
    /// native controller from the exited browser-process generation. The
    /// shell may recreate an id and must not issue a second id-only close.
    ProfileProcessExited {
        profile: ProfileId,
        ids: Vec<ItemId>,
    },
    /// The exact native-view generation was revoked and physically removed
    /// before this event was emitted. The shell may recreate `id` and must not
    /// send a cleanup close that could race the replacement generation.
    Crashed {
        id: ItemId,
    },
    Captured {
        id: ItemId,
        png: Vec<u8>,
    },
    HtmlExtracted {
        id: ItemId,
        html: String,
        truncated: bool,
    },
    SplitChanged {
        window: WindowId,
        tree: Pane,
    },
    ShortcutPressed {
        item: ItemId,
        command: String,
    },
}

#[cfg(test)]
mod user_content_tests {
    use super::*;

    fn script(id: u128) -> UserScript {
        let principal = ScriptPrincipal::Userscript(ScriptPrincipalId::from(id + 1_000));
        UserScript {
            id: ScriptId::from(id),
            owner: ScriptOwner::Principal(principal),
            source: "document.documentElement.dataset.zephium = '1'".into(),
            world: World::Isolated(principal),
            matches: MatchSet::all_urls(),
            run_at: RunAt::DocumentStart,
            all_frames: false,
        }
    }

    #[test]
    fn user_content_rejects_cross_principal_worlds() {
        let mut candidate = script(1);
        candidate.world =
            World::Isolated(ScriptPrincipal::Userscript(ScriptPrincipalId::from(9_999)));
        let failure = UserContent {
            scripts: vec![candidate],
            styles: Vec::new(),
        }
        .validate()
        .unwrap_err();
        assert!(matches!(
            failure,
            UserContentApplyFailure::Scripts(refusals)
                if refusals == vec![UserScriptRefusal {
                    registration: script(1).key(),
                    reason: UserScriptRefusalReason::OwnerWorldMismatch,
                }]
        ));
    }

    #[test]
    fn user_content_rejects_duplicate_ids_across_scripts_and_styles() {
        let script = script(2);
        let failure = UserContent {
            scripts: vec![script.clone()],
            styles: vec![UserStyle {
                id: ScriptId::from(2),
                owner: script.owner,
                css: "html { color: black; }".into(),
                matches: MatchSet::all_urls(),
                all_frames: false,
            }],
        }
        .validate()
        .unwrap_err();
        assert!(matches!(
            failure,
            UserContentApplyFailure::Scripts(refusals)
                if refusals.iter().any(|refusal| refusal.registration == script.key()
                    && refusal.reason == UserScriptRefusalReason::DuplicateId)
        ));
    }

    #[test]
    fn user_content_rejects_embedded_nul_before_native_conversion() {
        let mut script = script(3);
        script.source = "prefix\0suffix".into();
        let style = UserStyle {
            id: ScriptId::from(4),
            owner: script.owner,
            css: "html { color: red; }\0html { color: green; }".into(),
            matches: MatchSet::all_urls(),
            all_frames: false,
        };
        let expected = vec![
            UserScriptRefusal {
                registration: script.key(),
                reason: UserScriptRefusalReason::EmbeddedNul,
            },
            UserScriptRefusal {
                registration: style.key(),
                reason: UserScriptRefusalReason::EmbeddedNul,
            },
        ];

        let failure = UserContent {
            scripts: vec![script],
            styles: vec![style],
        }
        .validate()
        .unwrap_err();

        assert_eq!(failure, UserContentApplyFailure::Scripts(expected));
    }

    #[test]
    fn user_content_checks_count_before_walking_untrusted_entries() {
        let content = UserContent {
            scripts: (0..=MAX_USER_SCRIPTS_PER_SCOPE)
                .map(|index| script(index as u128 + 10))
                .collect(),
            styles: Vec::new(),
        };
        assert_eq!(
            content.validate(),
            Err(UserContentApplyFailure::TooManyScripts)
        );
    }

    #[test]
    fn script_identity_is_owner_qualified() {
        let first = script(50);
        let mut second = script(50);
        let second_principal = ScriptPrincipal::Extension(ScriptPrincipalId::from(99_999));
        second.owner = ScriptOwner::Principal(second_principal);
        second.world = World::Isolated(second_principal);
        assert_ne!(first.key(), second.key());
        assert!(UserContent {
            scripts: vec![first, second],
            styles: Vec::new(),
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn per_owner_script_count_is_bounded_below_scope_count() {
        let principal = ScriptPrincipal::Userscript(ScriptPrincipalId::from(50_000));
        let content = UserContent {
            scripts: (0..=MAX_USER_SCRIPTS_PER_OWNER)
                .map(|index| {
                    let mut script = script(index as u128 + 1_000);
                    script.owner = ScriptOwner::Principal(principal);
                    script.world = World::Isolated(principal);
                    script
                })
                .collect(),
            styles: Vec::new(),
        };
        assert_eq!(
            content.validate(),
            Err(UserContentApplyFailure::OwnerBudgetExceeded)
        );
    }

    #[test]
    fn retained_budget_counts_compiled_patterns_and_css_expansion() {
        let principal = ScriptPrincipal::Userscript(ScriptPrincipalId::from(123));
        let styles = (0..12)
            .map(|index| UserStyle {
                id: ScriptId::from(index + 10_000),
                owner: ScriptOwner::Principal(principal),
                css: std::iter::repeat_n('\u{1f}', MAX_USER_STYLE_BYTES)
                    .collect::<String>()
                    .into(),
                matches: MatchSet::all_urls(),
                all_frames: false,
            })
            .collect();
        let content = UserContent {
            scripts: Vec::new(),
            styles,
        };
        assert_eq!(
            content.validate(),
            Err(UserContentApplyFailure::OwnerBudgetExceeded)
        );
    }
}
