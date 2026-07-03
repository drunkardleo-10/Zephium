use crate::geometry::Rect;
use crate::ids::{ItemId, ProfileId, WindowId};
use crate::split::Pane;

/// Which engine data partition a view lives in. Default profile shares the
/// OS default store; named profiles get their own persistent partition;
/// incognito is ephemeral and never touches disk.
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
    fn create_view(&self, id: ItemId, partition: Partition, url: &str, bounds: Rect);
    fn navigate(&self, id: ItemId, url: &str);
    fn reload(&self, id: ItemId);
    fn stop(&self, id: ItemId);
    fn go_back(&self, id: ItemId);
    fn go_forward(&self, id: ItemId);
    fn close(&self, id: ItemId);
    /// Lay the split `tree` into `region` of `window`, or hide that window's
    /// content when `region` is `None`. The engine owns pane geometry so
    /// resize stays in the native pass.
    fn set_content(&self, window: WindowId, tree: Option<Pane>, region: Option<Rect>);
    fn set_drop_indicator(&self, window: WindowId, zone: Option<Rect>);
    fn zoom(&self, id: ItemId, scale: f64);
    fn set_muted(&self, id: ItemId, muted: bool);
    /// `None` clears the current find session.
    fn find(&self, id: ItemId, query: Option<&str>);
    /// Result arrives as `EngineEvent::Captured`.
    fn capture(&self, id: ItemId);
    /// Result arrives as `EngineEvent::HtmlExtracted`.
    fn extract_html(&self, id: ItemId);
    /// Asks the page for its best icon link; result arrives as
    /// `EngineEvent::FaviconChanged` after validation.
    fn discover_favicon(&self, id: ItemId);
    fn print(&self, id: ItemId);
    fn set_user_content(&self, scope: ContentScope, content: UserContent);
    fn set_shortcuts(&self, shortcuts: Vec<Shortcut>);
    /// Compiled rule payload; format is engine-specific (WebKit JSON,
    /// WebView2 filter set).
    fn set_content_rules(&self, profile: ProfileId, compiled: String);
}

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
    TitleChanged {
        id: ItemId,
        title: String,
    },
    UrlChanged {
        id: ItemId,
        url: String,
    },
    LoadingChanged {
        id: ItemId,
        loading: bool,
    },
    FaviconChanged {
        id: ItemId,
        urls: Vec<String>,
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
    },
    SplitChanged {
        window: WindowId,
        tree: Pane,
    },
    ShortcutPressed {
        id: String,
    },
}
