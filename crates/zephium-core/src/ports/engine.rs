use crate::geometry::Rect;
use crate::ids::{ItemId, ProfileId, WindowId};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum World {
    Page,
    Isolated,
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
#[derive(Clone, Debug, Default)]
pub struct UserContent {
    pub scripts: Vec<UserScript>,
    pub styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct UserScript {
    pub source: String,
    pub world: World,
    pub at_start: bool,
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
    /// request was not admitted. A request admitted by this method can still
    /// fail synchronously inside the native engine; that arrives as
    /// `EngineEvent::NavigationFailed` with the same request id.
    fn navigate(&self, id: ItemId, url: &str, request: NavigationRequestId) -> bool;
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
    fn zoom(&self, id: ItemId, scale: f64) -> NativeDispatch;
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
    fn set_user_content(&self, scope: ContentScope, content: UserContent);
    fn set_shortcuts(&self, shortcuts: Vec<Shortcut>);
    /// Prebuild a hidden webview for `partition` so the next open adopts it
    /// instead of paying the renderer spawn. Safe moment: after a page load.
    fn warm_spare(&self, _partition: Partition) {}
    /// Hidden views the shell's idle policy wants suspended. The engine
    /// suspends where it has a primitive (WebView2) and resumes implicitly
    /// when a view becomes visible again.
    fn set_dormant(&self, _ids: Vec<ItemId>) {}
    /// Compiled rule payload; format is engine-specific (WebKit JSON,
    /// WebView2 filter set).
    fn set_content_rules(&self, profile: ProfileId, compiled: String);
    /// Sticky process-local signal that the native browser runtime reported a
    /// newer version. `true` never means the running environments adopted the
    /// update: the composition root must use its ordinary ordered shutdown
    /// path and perform a whole-application restart, including privileged
    /// chrome, before clearing this state.
    fn runtime_restart_required(&self) -> bool {
        false
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

/// Correlates an explicit shell navigation with an immediate native failure.
///
/// Successful commits intentionally do not carry this token: page-initiated
/// navigations, redirects and same-document history changes have no shell
/// request and all use the same authoritative `UrlChanged` path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NavigationRequestId(pub u64);

/// Process-local correlation identity for one renderer-state discard probe.
/// It is never persisted or accepted from page content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiscardProbeId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionKind {
    Geolocation,
    Camera,
    Microphone,
    Notifications,
    ClipboardRead,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// A native browser environment reported that a newer runtime is
    /// available. This is a process-global, sticky notification: recycling a
    /// content profile alone cannot update privileged chrome and must not be
    /// presented as successful adoption.
    RuntimeRestartRequired,
    TitleChanged {
        id: ItemId,
        title: String,
    },
    UrlChanged {
        id: ItemId,
        url: String,
    },
    NavigationFailed {
        id: ItemId,
        request: NavigationRequestId,
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
