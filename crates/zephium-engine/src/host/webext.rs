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

use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly};
use objc2_foundation::{NSObjectProtocol, NSURLRequest, NSURL};
use objc2_web_kit::{WKNavigationDelegate, WKUIDelegate};

use super::permits::Sink;

#[derive(Default)]
pub(crate) struct WebextHost {
    profiles: HashMap<ProfileId, ProfileRuntime>,
    pages: HashMap<ItemId, Page>,
}

/// An extension page shown in an extension-owned tab.
struct Page {
    profile: ProfileId,
    view: Retained<WKWebView>,
    stage: Retained<crate::platform::imp::ContentStage>,
    _delegate: Retained<PageDelegate>,
}

pub(crate) enum BrowserRequestOutcome {
    NotOurs,
    Settled,
    /// The shell made an extension-owned tab; the page still has to be put
    /// into it.
    Page {
        extension_id: String,
        url: String,
        done: Option<TabRequestDone>,
    },
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
                pending_pages: RefCell::new(HashMap::new()),
                next_request: Cell::new(1),
                popup: RefCell::new(None),
                repeats: RefCell::new(HashMap::new()),
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
        // The engine's own views are the truth for residency; the shell's flag
        // trails view creation and would unbind a view bound on insertion.
        for tab in surface.tabs() {
            let id = entry.bridge.ids.borrow_mut().tab(tab.id());
            let view = view_for(tab.id());
            if zephium_webext_macos::tracing() {
                eprintln!(
                    "webext-trace: publish tab {id} resident={} view={} url={:?}",
                    tab.resident(),
                    view.is_some(),
                    tab.url().map(|u| u.split('?').next().unwrap_or(u))
                );
            }
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
            let badge = truncate(
                &badge,
                zephium_core::extensions::MAX_EXTENSION_ACTION_BADGE_BYTES,
            );
            // WebKit keeps the unread flag after an extension clears its badge.
            let unread = unsafe { action.hasUnreadBadgeText() } && !badge.is_empty();
            match ExtensionActionState::new(
                runtime,
                ExtensionActionScope::Tab(tab),
                revision,
                truncate(
                    &label,
                    zephium_core::extensions::MAX_EXTENSION_ACTION_LABEL_BYTES,
                ),
                badge,
                icon,
                unsafe { action.isEnabled() },
                unsafe { action.presentsPopup() },
                unread,
            ) {
                Ok(state) => actions.push(state),
                Err(error) => {
                    eprintln!(
                        "extensions: {} action not shown: {error:?}",
                        install.extension_id
                    )
                }
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
    ) -> BrowserRequestOutcome {
        let Some(entry) = self.profiles.get(&profile) else {
            return BrowserRequestOutcome::NotOurs;
        };
        if let Some(page) = entry.bridge.pending_pages.borrow_mut().remove(&request) {
            return match settlement {
                ExtensionBrowserRequestSettlement::Applied(
                    ExtensionBrowserRequestResult::ExtensionPageAuthorized { .. },
                ) => BrowserRequestOutcome::Page {
                    extension_id: page.extension_id,
                    url: page.url,
                    done: page.done,
                },
                _ => {
                    if let Some(done) = page.done {
                        done(Err("The browser declined to open the page.".into()));
                    }
                    BrowserRequestOutcome::Settled
                }
            };
        }
        let Some(done) = entry.bridge.pending.borrow_mut().remove(&request) else {
            return BrowserRequestOutcome::NotOurs;
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
        BrowserRequestOutcome::Settled
    }

    /// Shows an extension page in the tab the shell created for it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn present_page(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
        stage: Retained<crate::platform::imp::ContentStage>,
        permit: Arc<std::sync::atomic::AtomicBool>,
        extension_id: &str,
        url: &str,
        sink: &Sink,
    ) -> Result<(), String> {
        let entry = self
            .profiles
            .get(&profile)
            .ok_or("no runtime for this profile")?;
        let context = entry
            .runtime
            .context(extension_id)
            .ok_or("the extension is not running")?;
        let configuration = unsafe { context.webViewConfiguration() }
            .ok_or("the extension has no page configuration")?;
        let mtm = MainThreadMarker::new().ok_or("main thread required")?;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(960.0, 720.0));
        let view = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };
        let origin = format!("chrome-extension://{extension_id}/");
        let delegate = PageDelegate::new(
            mtm,
            profile,
            tab,
            origin,
            sink.clone(),
            Rc::downgrade(&entry.bridge),
        );
        unsafe {
            view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            view.setUIDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            view.setInspectable(true);
        }
        let url = NSURL::URLWithString(&NSString::from_str(url)).ok_or("invalid page address")?;
        if !stage.insert_view(tab, Retained::into_super(view.clone()), permit) {
            return Err("the tab could not take the page".into());
        }
        unsafe { view.loadRequest(&NSURLRequest::requestWithURL(&url)) };
        let _ = stage.set_ready(tab);
        let id = entry.bridge.ids.borrow_mut().tab(tab);
        entry.runtime.bind_view(id, Some(&view));
        self.pages.insert(
            tab,
            Page {
                profile,
                view,
                stage,
                _delegate: delegate,
            },
        );
        Ok(())
    }

    pub(crate) fn tab_number(&self, profile: ProfileId, tab: ItemId) -> u64 {
        self.profiles
            .get(&profile)
            .map_or(0, |entry| entry.bridge.ids.borrow_mut().tab(tab))
    }

    /// Closes an extension page; false when `tab` shows none.
    pub(crate) fn close_page(&mut self, tab: ItemId) -> bool {
        let Some(page) = self.pages.remove(&tab) else {
            return false;
        };
        unsafe {
            page.view.stopLoading();
            page.view.setNavigationDelegate(None);
            page.view.setUIDelegate(None);
        }
        page.stage.remove_view(tab);
        if let Some(entry) = self.profiles.get(&page.profile) {
            let id = entry.bridge.ids.borrow_mut().tab(tab);
            entry.runtime.bind_view(id, None);
        }
        true
    }

    /// Reload (0), back (1), forward (2) or stop (3) on an extension page.
    pub(crate) fn navigate_page(&self, tab: ItemId, action: u8) -> bool {
        let Some(page) = self.pages.get(&tab) else {
            return false;
        };
        unsafe {
            match action {
                0 => drop(page.view.reload()),
                1 => drop(page.view.goBack()),
                2 => drop(page.view.goForward()),
                _ => page.view.stopLoading(),
            }
        }
        true
    }

    pub(crate) fn open_options(&mut self, runtime: ExtensionRuntimeInstance) -> bool {
        let Some(entry) = self.profiles.get(&runtime.profile()) else {
            return false;
        };
        let Some(install) = entry.installs.get(&runtime.install_id()) else {
            return false;
        };
        let Some(url) = entry
            .runtime
            .context(&install.extension_id)
            .and_then(|context| unsafe { context.optionsPageURL() })
            .and_then(|url| url.absoluteString())
        else {
            return false;
        };
        entry
            .bridge
            .open_page(install.extension_id.clone(), url.to_string(), None);
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
    pending_pages: RefCell<HashMap<ExtensionBrowserRequestId, PendingPage>>,
    next_request: Cell<u64>,
    popup: RefCell<Option<(Retained<NSView>, Rect)>>,
    repeats: RefCell<HashMap<u64, (std::time::Instant, u32)>>,
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

    /// Asks the shell for an extension-owned tab showing one of an
    /// extension's own pages; WebKit serves those pages only to views built
    /// from that extension's configuration.
    fn open_page(&self, extension_id: String, url: String, done: Option<TabRequestDone>) {
        let number = self.next_request.get();
        self.next_request.set(number + 1);
        let Some(id) = ExtensionBrowserRequestId::new(number | (1 << 63)) else {
            if let Some(done) = done {
                done(Err("request identity exhausted".into()));
            }
            return;
        };
        match ExtensionBrowserRequest::new(
            self.profile,
            id,
            ExtensionBrowserRequestAction::OpenExtensionPage,
        ) {
            Ok(request) => {
                self.pending_pages.borrow_mut().insert(
                    id,
                    PendingPage {
                        extension_id,
                        url,
                        done,
                    },
                );
                self.sink
                    .emit(EngineEvent::ExtensionBrowserRequested { request });
            }
            Err(_) => {
                if let Some(done) = done {
                    done(Err("The page cannot be opened.".into()));
                }
            }
        }
    }
}

struct PendingPage {
    extension_id: String,
    url: String,
    done: Option<TabRequestDone>,
}

/// The extension a `chrome-extension://<id>/…` address belongs to.
fn extension_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix(concat!("chrome-extension", "://"))?;
    let id = rest.split('/').next()?;
    (id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))).then_some(id)
}

impl Host for Bridge {
    fn log(&self, extension: &str, level: LogLevel, message: &str) {
        // Extensions retry failing calls in loops; print each distinct line
        // once a minute with how often it repeated.
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (extension, message).hash(&mut hasher);
        let now = std::time::Instant::now();
        let repeated = {
            let mut repeats = self.repeats.borrow_mut();
            if repeats.len() > 512 {
                repeats.retain(|_, (since, _)| now.duration_since(*since).as_secs() < 60);
            }
            let entry = repeats.entry(hasher.finish()).or_insert((now, 0));
            if entry.1 > 0 && now.duration_since(entry.0).as_secs() < 60 {
                entry.1 += 1;
                return;
            }
            let repeated = entry.1.saturating_sub(1);
            *entry = (now, 1);
            repeated
        };
        let suffix = if repeated > 0 {
            format!(" (repeated {repeated} more times)")
        } else {
            String::new()
        };
        let tag = match level {
            LogLevel::Info => "info",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
        };
        eprintln!("extension {extension} {tag}: {message}{suffix}");
    }

    fn tab_request(&self, request: TabRequest, done: TabRequestDone) {
        let tab = |id: u64| self.item(id);
        match &request {
            TabRequest::Create { url: Some(url), .. } => {
                if let Some(extension) = extension_of(url) {
                    return self.open_page(extension.to_owned(), url.clone(), Some(done));
                }
            }
            TabRequest::Load { tab: id, url } => {
                if let (Some(extension), Some(item)) = (extension_of(url), tab(*id)) {
                    // A website sending its tab to an extension page (1Password's
                    // sign-in) gets the page in an extension-owned tab instead.
                    let extension = extension.to_owned();
                    let url = url.clone();
                    self.request(
                        ExtensionBrowserRequestAction::CloseTab { tab: item },
                        Box::new(|_| {}),
                    );
                    return self.open_page(extension, url, Some(done));
                }
            }
            _ => {}
        }
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

    fn open_options(&self, extension: &str, url: &str) {
        self.open_page(extension.to_owned(), url.to_owned(), None);
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

struct PageIvars {
    profile: ProfileId,
    tab: ItemId,
    origin: String,
    sink: Sink,
    bridge: std::rc::Weak<Bridge>,
}

objc2::define_class!(
    #[unsafe(super(objc2::runtime::NSObject))]
    #[thread_kind = objc2::MainThreadOnly]
    #[name = "ZephiumWebExtPageDelegate"]
    #[ivars = PageIvars]
    struct PageDelegate;

    unsafe impl NSObjectProtocol for PageDelegate {}

    unsafe impl WKNavigationDelegate for PageDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            _view: &WKWebView,
            action: &objc2_web_kit::WKNavigationAction,
            decision: &block2::DynBlock<dyn Fn(objc2_web_kit::WKNavigationActionPolicy)>,
        ) {
            use objc2_web_kit::WKNavigationActionPolicy as Policy;
            let url = unsafe { action.request() }
                .URL()
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string())
                .unwrap_or_default();
            let main_frame =
                unsafe { action.targetFrame() }.is_some_and(|frame| unsafe { frame.isMainFrame() });
            // An extension-owned tab only shows its extension's pages; leaving
            // for the web continues in a normal tab.
            if main_frame && !url.starts_with(&self.ivars().origin) && url.starts_with("http") {
                decision.call((Policy::Cancel,));
                if let Some(bridge) = self.ivars().bridge.upgrade() {
                    bridge.request(
                        ExtensionBrowserRequestAction::CreateTab {
                            window: None,
                            url: Some(Arc::from(url.as_str())),
                            active: true,
                        },
                        Box::new(|_| {}),
                    );
                }
                return;
            }
            decision.call((Policy::Allow,));
        }

        #[unsafe(method(webView:didCommitNavigation:))]
        fn did_commit(&self, view: &WKWebView, _navigation: Option<&objc2_web_kit::WKNavigation>) {
            self.changed(view);
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, view: &WKWebView, _navigation: Option<&objc2_web_kit::WKNavigation>) {
            self.changed(view);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            view: &WKWebView,
            _navigation: Option<&objc2_web_kit::WKNavigation>,
            _error: &objc2_foundation::NSError,
        ) {
            self.changed(view);
        }
    }

    unsafe impl WKUIDelegate for PageDelegate {
        #[unsafe(method(webViewDidClose:))]
        fn did_close(&self, _view: &WKWebView) {
            let ivars = self.ivars();
            ivars.sink.emit(EngineEvent::ExtensionPageClosed {
                profile: ivars.profile,
                id: ivars.tab,
            });
        }

        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_view(
            &self,
            _view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &objc2_web_kit::WKNavigationAction,
            _features: &objc2_web_kit::WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            let url: Option<String> = unsafe { action.request() }
                .URL()
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string());
            if let (Some(url), Some(bridge)) = (url, self.ivars().bridge.upgrade()) {
                match extension_of(&url) {
                    Some(extension) => bridge.open_page(extension.to_owned(), url, None),
                    None => bridge.request(
                        ExtensionBrowserRequestAction::CreateTab {
                            window: None,
                            url: Some(Arc::from(url.as_str())),
                            active: true,
                        },
                        Box::new(|_| {}),
                    ),
                }
            }
            None
        }
    }
);

impl PageDelegate {
    fn new(
        mtm: MainThreadMarker,
        profile: ProfileId,
        tab: ItemId,
        origin: String,
        sink: Sink,
        bridge: std::rc::Weak<Bridge>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PageIvars {
            profile,
            tab,
            origin,
            sink,
            bridge,
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    fn changed(&self, view: &WKWebView) {
        let ivars = self.ivars();
        let title = unsafe { view.title() }
            .map(|title| title.to_string())
            .unwrap_or_default();
        ivars.sink.emit(EngineEvent::ExtensionPageChanged {
            profile: ivars.profile,
            id: ivars.tab,
            title: zephium_core::item::sanitize_page_title(&title),
            loading: unsafe { view.isLoading() },
            can_go_back: unsafe { view.canGoBack() },
            can_go_forward: unsafe { view.canGoForward() },
        });
    }
}
