//! The browser side of the WebKit extension runtime: one runtime per durable
//! profile, attached to every tab of that profile from its first view.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSPopoverBehavior, NSView};
use objc2_foundation::{NSPoint, NSRect, NSRectEdge, NSSize, NSString};
use objc2_web_kit::{WKWebExtensionAction, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore};
use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionRequest, ExtensionActionRevision,
    ExtensionActionScope, ExtensionActionSettlement, ExtensionActionSnapshot,
    ExtensionActionSnapshotSettlement, ExtensionActionState, ExtensionBrowserRequest,
    ExtensionBrowserRequestAction, ExtensionBrowserRequestId, ExtensionBrowserRequestResult,
    ExtensionBrowserRequestSettlement, ExtensionBrowserSurface, ExtensionBrowserSurfaceGeneration,
    ExtensionRuntimeGeneration, ExtensionRuntimeInstance,
};
use zephium_core::geometry::Rect;
use zephium_core::ids::{ExtensionInstallId, ItemId, ProfileId, WindowId};
use zephium_core::ports::engine::{EngineEvent, WebExtensionLoad, WebExtensionLoaded};
use zephium_webext_macos::{
    ExtensionSpec, Grants, Host, LogLevel, Runtime, TabRequest, TabRequestDone, TabSnapshot,
    WindowSnapshot,
};

use super::permits::Sink;

#[derive(Default)]
pub(crate) struct WebextHost {
    profiles: HashMap<ProfileId, ProfileRuntime>,
}

struct ProfileRuntime {
    runtime: Runtime,
    store: Retained<WKWebsiteDataStore>,
    bridge: Rc<Bridge>,
    installs: HashMap<ExtensionInstallId, Install>,
    surface: Option<ExtensionBrowserSurface>,
}

struct Install {
    extension_id: String,
    generation: ExtensionRuntimeGeneration,
    loaded: bool,
}

impl WebextHost {
    /// A fresh configuration for one of the profile's tabs, attached to its
    /// runtime. The runtime is created with the profile's first view, so
    /// every tab can run content scripts.
    pub(crate) fn configuration(
        &mut self,
        profile: ProfileId,
        sink: &Sink,
    ) -> Retained<WKWebViewConfiguration> {
        let mtm = MainThreadMarker::new().expect("engine host runs on the main thread");
        let entry = self.profile(profile, sink);
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe { configuration.setWebsiteDataStore(&entry.store) };
        entry.runtime.configure(&configuration);
        configuration
    }

    fn profile(&mut self, profile: ProfileId, sink: &Sink) -> &mut ProfileRuntime {
        self.profiles.entry(profile).or_insert_with(|| {
            let mtm = MainThreadMarker::new().expect("engine host runs on the main thread");
            let identifier = profile_uuid(profile);
            let store = unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm) };
            let bridge = Rc::new(Bridge {
                profile,
                sink: sink.clone(),
                ids: RefCell::new(IdMap::default()),
                pending: RefCell::new(HashMap::new()),
                next_request: Cell::new(1),
                popup: RefCell::new(None),
            });
            let runtime = Runtime::new(mtm, &store, Some(&identifier), bridge.clone());
            ProfileRuntime {
                runtime,
                store,
                bridge,
                installs: HashMap::new(),
                surface: None,
            }
        })
    }

    pub(crate) fn load(&mut self, profile: ProfileId, load: WebExtensionLoad, sink: &Sink) {
        let entry = self.profile(profile, sink);
        if let Some(install) = entry.installs.remove(&load.install) {
            entry.runtime.unload(&install.extension_id);
        }
        let generation = entry
            .installs
            .values()
            .map(|install| install.generation.get())
            .max()
            .and_then(|generation| ExtensionRuntimeGeneration::new(generation + 1))
            .unwrap_or(ExtensionRuntimeGeneration::new(1).expect("nonzero"));
        entry.installs.insert(
            load.install,
            Install {
                extension_id: load.extension_id.clone(),
                generation,
                loaded: false,
            },
        );
        let spec = ExtensionSpec {
            id: load.extension_id.clone(),
            root: load.root,
            grants: Grants::Explicit {
                permissions: load.permissions,
                match_patterns: load.match_patterns,
            },
            inspectable: true,
        };
        let install = load.install;
        let start_background = load.start_background;
        let sink = sink.clone();
        entry.runtime.load(spec, move |result| {
            let result = result.map(|loaded| {
                let extension_id = loaded.id.clone();
                super::dispatch::best_effort_with(move |host| {
                    if let Some(entry) = host.webext.profiles.get_mut(&profile) {
                        if let Some(install) = entry.installs.get_mut(&install) {
                            install.loaded = true;
                        }
                        if start_background {
                            entry.runtime.start_background(&extension_id, |_| {});
                        }
                    }
                });
                WebExtensionLoaded {
                    name: loaded.name,
                    version: loaded.version,
                }
            });
            if let Err(error) = &result {
                eprintln!("extensions: {} failed to load: {error}", install);
            }
            sink.emit(EngineEvent::WebExtensionSettled {
                profile,
                install,
                result,
            });
            sink.emit(EngineEvent::ExtensionActionsInvalidated { profile });
        });
    }

    pub(crate) fn unload(&mut self, profile: ProfileId, install: ExtensionInstallId, sink: &Sink) {
        if let Some(entry) = self.profiles.get_mut(&profile) {
            if let Some(install) = entry.installs.remove(&install) {
                entry.runtime.unload(&install.extension_id);
                sink.emit(EngineEvent::ExtensionActionsInvalidated { profile });
            }
        }
    }

    /// Mirrors the shell's windows and tabs into WebKit.
    pub(crate) fn publish(
        &mut self,
        surface: &ExtensionBrowserSurface,
        view_for: impl Fn(ItemId) -> Option<Retained<WKWebView>>,
    ) {
        let Some(entry) = self.profiles.get_mut(&surface.profile()) else {
            return;
        };
        let windows: Vec<WindowSnapshot> = {
            let mut ids = entry.bridge.ids.borrow_mut();
            surface
                .windows()
                .iter()
                .filter(|window| !window.is_private())
                .map(|window| WindowSnapshot {
                    id: ids.window(window.id()),
                    tabs: window
                        .tabs()
                        .iter()
                        .map(|tab| TabSnapshot {
                            id: ids.tab(tab.id()),
                            title: tab.title().to_owned(),
                            url: tab.url().map(str::to_owned),
                            loading: tab.loading(),
                            pinned: tab.pinned(),
                        })
                        .collect(),
                    active: window.active().map(|tab| ids.tab(tab)),
                    frame: (0.0, 0.0, 1200.0, 800.0),
                })
                .collect()
        };
        let focused = surface
            .focused()
            .map(|window| entry.bridge.ids.borrow_mut().window(window));
        entry.runtime.publish(&windows, focused);
        for tab in surface.tabs() {
            let id = entry.bridge.ids.borrow_mut().tab(tab.id());
            let view = tab.resident().then(|| view_for(tab.id())).flatten();
            entry.runtime.bind_view(id, view.as_deref());
        }
        entry.surface = Some(surface.clone());
    }

    pub(crate) fn bind_view(&mut self, profile: ProfileId, tab: ItemId, view: Option<&WKWebView>) {
        if let Some(entry) = self.profiles.get_mut(&profile) {
            let id = entry.bridge.ids.borrow_mut().tab(tab);
            entry.runtime.bind_view(id, view);
        }
    }

    pub(crate) fn actions(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
        surface_generation: ExtensionBrowserSurfaceGeneration,
    ) -> ExtensionActionSnapshotSettlement {
        let Some(entry) = self.profiles.get_mut(&profile) else {
            return ExtensionActionSnapshotSettlement::Rejected(
                ExtensionActionRejection::RuntimeUnavailable,
            );
        };
        let tab_id = entry.bridge.ids.borrow_mut().tab(tab);
        let tab_object = entry.runtime.tab_object(tab_id);
        let mut actions = Vec::new();
        for (install_id, install) in &entry.installs {
            if !install.loaded {
                continue;
            }
            let Some(context) = entry.runtime.context(&install.extension_id) else {
                continue;
            };
            let Some(action) = (unsafe { context.actionForTab(tab_object.as_deref()) }) else {
                continue;
            };
            let label = unsafe { action.label() }.to_string();
            let badge = unsafe { action.badgeText() }.to_string();
            let icon = crate::platform::imp::rasterize_webext_action_icon(&action);
            let revision = presentation_revision(&label, &badge, icon.as_ref().map(|i| i.rgba()));
            let runtime = ExtensionRuntimeInstance::new(profile, *install_id, install.generation);
            if let Ok(state) = ExtensionActionState::new(
                runtime,
                ExtensionActionScope::Tab(tab),
                revision,
                truncate(
                    &label,
                    zephium_core::extensions::MAX_EXTENSION_ACTION_LABEL_BYTES,
                ),
                truncate(
                    &badge,
                    zephium_core::extensions::MAX_EXTENSION_ACTION_BADGE_BYTES,
                ),
                icon,
                unsafe { action.isEnabled() },
                unsafe { action.presentsPopup() },
                unsafe { action.hasUnreadBadgeText() },
            ) {
                actions.push(state);
            }
        }
        actions.sort_by_key(|action| action.runtime().install_id());
        match ExtensionActionSnapshot::new(profile, tab, surface_generation, actions) {
            Ok(snapshot) => ExtensionActionSnapshotSettlement::Applied(snapshot),
            Err(_) => ExtensionActionSnapshotSettlement::Rejected(
                ExtensionActionRejection::NativeAdmissionFailed,
            ),
        }
    }

    /// Runs a toolbar action. A popup is presented from WebKit's delegate
    /// callback, anchored to the button the user clicked.
    pub(crate) fn invoke(
        &mut self,
        request: ExtensionActionRequest,
        parent: Option<Retained<NSView>>,
    ) -> ExtensionActionSettlement {
        let profile = request.runtime().profile();
        let Some(entry) = self.profiles.get_mut(&profile) else {
            return ExtensionActionSettlement::Rejected(
                ExtensionActionRejection::RuntimeUnavailable,
            );
        };
        let Some(install) = entry.installs.get(&request.runtime().install_id()) else {
            return ExtensionActionSettlement::Rejected(
                ExtensionActionRejection::RuntimeUnavailable,
            );
        };
        let extension_id = install.extension_id.clone();
        if let Some(parent) = parent {
            *entry.bridge.popup.borrow_mut() = Some((parent, request.anchor().rect()));
        }
        let tab = entry.bridge.ids.borrow_mut().tab(request.tab());
        if entry.runtime.perform_action(&extension_id, Some(tab)) {
            ExtensionActionSettlement::Dispatched
        } else {
            ExtensionActionSettlement::Rejected(ExtensionActionRejection::RuntimeUnavailable)
        }
    }

    /// Completes an extension's tab request with the shell's answer. Returns
    /// false when the request is not one of this runtime's.
    pub(crate) fn settle_browser_request(
        &mut self,
        profile: ProfileId,
        request: ExtensionBrowserRequestId,
        settlement: ExtensionBrowserRequestSettlement,
    ) -> bool {
        let Some(entry) = self.profiles.get(&profile) else {
            return false;
        };
        let Some(done) = entry.bridge.pending.borrow_mut().remove(&request) else {
            return false;
        };
        match settlement {
            ExtensionBrowserRequestSettlement::Applied(
                ExtensionBrowserRequestResult::CreatedTab(tab),
            ) => {
                let id = entry.bridge.ids.borrow_mut().tab(tab);
                done(Ok(Some(id)));
            }
            ExtensionBrowserRequestSettlement::Applied(_) => done(Ok(None)),
            ExtensionBrowserRequestSettlement::Rejected(rejection) => done(Err(format!(
                "The browser declined the request ({rejection:?})."
            ))),
        }
        true
    }
}

fn presentation_revision(label: &str, badge: &str, icon: Option<&[u8]>) -> ExtensionActionRevision {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (label, badge, icon).hash(&mut hasher);
    ExtensionActionRevision::new(hasher.finish().max(1)).expect("nonzero")
}

fn truncate(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn profile_uuid(profile: ProfileId) -> Retained<objc2_foundation::NSUUID> {
    let bytes = profile.bytes();
    let text = format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    );
    objc2_foundation::NSUUID::from_string(&NSString::from_str(&text)).expect("formatted UUID")
}

/// Stable runtime-local numbers for the shell's tab and window identities.
#[derive(Default)]
struct IdMap {
    tabs: HashMap<ItemId, u64>,
    tabs_back: HashMap<u64, ItemId>,
    windows: HashMap<WindowId, u64>,
    windows_back: HashMap<u64, WindowId>,
    next: u64,
}

impl IdMap {
    fn tab(&mut self, id: ItemId) -> u64 {
        if let Some(number) = self.tabs.get(&id) {
            return *number;
        }
        self.next += 1;
        self.tabs.insert(id, self.next);
        self.tabs_back.insert(self.next, id);
        self.next
    }

    fn window(&mut self, id: WindowId) -> u64 {
        if let Some(number) = self.windows.get(&id) {
            return *number;
        }
        self.next += 1;
        self.windows.insert(id, self.next);
        self.windows_back.insert(self.next, id);
        self.next
    }
}

struct Bridge {
    profile: ProfileId,
    sink: Sink,
    ids: RefCell<IdMap>,
    pending: RefCell<HashMap<ExtensionBrowserRequestId, TabRequestDone>>,
    next_request: Cell<u64>,
    popup: RefCell<Option<(Retained<NSView>, Rect)>>,
}

impl Bridge {
    fn request(&self, action: ExtensionBrowserRequestAction, done: TabRequestDone) {
        let number = self.next_request.get();
        self.next_request.set(number + 1);
        // The high bit keeps these apart from the previous runtime's ids.
        let Some(id) = ExtensionBrowserRequestId::new(number | (1 << 63)) else {
            return done(Err("request identity exhausted".into()));
        };
        match ExtensionBrowserRequest::new(self.profile, id, action) {
            Ok(request) => {
                self.pending.borrow_mut().insert(id, done);
                self.sink
                    .emit(EngineEvent::ExtensionBrowserRequested { request });
            }
            Err(_) => done(Err("That address cannot be opened.".into())),
        }
    }

    fn item(&self, tab: u64) -> Option<ItemId> {
        self.ids.borrow().tabs_back.get(&tab).copied()
    }
}

impl Host for Bridge {
    fn log(&self, extension: &str, level: LogLevel, message: &str) {
        let tag = match level {
            LogLevel::Info => "info",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
        };
        eprintln!("extension {extension} {tag}: {message}");
    }

    fn tab_request(&self, request: TabRequest, done: TabRequestDone) {
        let tab = |id: u64| self.item(id);
        let action = match request {
            TabRequest::Create {
                window,
                url,
                active,
                ..
            } => ExtensionBrowserRequestAction::CreateTab {
                window: window.and_then(|w| self.ids.borrow().windows_back.get(&w).copied()),
                url: url.filter(|url| url != "about:blank").map(Arc::from),
                active,
            },
            TabRequest::Activate { tab: id } => match tab(id) {
                Some(tab) => ExtensionBrowserRequestAction::ActivateTab { tab },
                None => return done(Err("No tab with that id.".into())),
            },
            TabRequest::Close { tab: id } => match tab(id) {
                Some(tab) => ExtensionBrowserRequestAction::CloseTab { tab },
                None => return done(Err("No tab with that id.".into())),
            },
            TabRequest::Load { tab: id, url } => match tab(id) {
                Some(tab) => ExtensionBrowserRequestAction::LoadTabUrl {
                    tab,
                    url: Arc::from(url),
                },
                None => return done(Err("No tab with that id.".into())),
            },
            TabRequest::Reload { tab: id, .. } => match tab(id) {
                Some(tab) => ExtensionBrowserRequestAction::ReloadTab { tab },
                None => return done(Err("No tab with that id.".into())),
            },
            TabRequest::Back { tab: id } => match tab(id) {
                Some(tab) => ExtensionBrowserRequestAction::GoBack { tab },
                None => return done(Err("No tab with that id.".into())),
            },
            TabRequest::Forward { tab: id } => match tab(id) {
                Some(tab) => ExtensionBrowserRequestAction::GoForward { tab },
                None => return done(Err("No tab with that id.".into())),
            },
            TabRequest::Pin { .. } | TabRequest::FocusWindow { .. } => {
                return done(Ok(None));
            }
        };
        self.request(action, done);
    }

    fn present_popup(&self, _extension: &str, action: &WKWebExtensionAction) -> bool {
        let Some((parent, anchor)) = self.popup.borrow_mut().take() else {
            return false;
        };
        let Some(popover) = (unsafe { action.popupPopover() }) else {
            return false;
        };
        let bounds = parent.bounds();
        let flipped = parent.isFlipped();
        let y = if flipped {
            anchor.y
        } else {
            bounds.size.height - anchor.y - anchor.height
        };
        let rect = NSRect::new(
            NSPoint::new(anchor.x, y),
            NSSize::new(anchor.width.max(1.0), anchor.height.max(1.0)),
        );
        let edge = if flipped {
            NSRectEdge::MaxY
        } else {
            NSRectEdge::MinY
        };
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.showRelativeToRect_ofView_preferredEdge(rect, &parent, edge);
        popover.isShown()
    }
}

impl super::EngineHost {
    pub(crate) fn load_web_extension(&mut self, profile: ProfileId, load: WebExtensionLoad) {
        let sink = self.sink.clone();
        self.webext.load(profile, load, &sink);
    }

    pub(crate) fn unload_web_extension(&mut self, profile: ProfileId, install: ExtensionInstallId) {
        let sink = self.sink.clone();
        self.webext.unload(profile, install, &sink);
    }
}
