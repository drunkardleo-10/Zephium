use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Once;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSError, NSString, NSURL, NSUUID};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionContextPermissionStatus,
    WKWebExtensionController, WKWebExtensionControllerConfiguration, WKWebExtensionMatchPattern,
    WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};

use crate::delegate::Delegate;
use crate::surface::{Graph, Tab, Window, WindowSnapshot};
use crate::{application_name, Host, LogLevel};

/// Chrome's scheme, so extension origins (and everything stored under them)
/// match what servers and the extension itself expect.
pub(crate) const SCHEME: &str = "chrome-extension";

/// What the user consented to for one extension.
#[derive(Clone, Debug)]
pub enum Grants {
    /// Everything the manifest requires, as accepted at install.
    Requested,
    Explicit {
        permissions: Vec<String>,
        match_patterns: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub struct ExtensionSpec {
    /// The Chrome Web Store ID; it is also the origin host.
    pub id: String,
    /// A prepared package directory.
    pub root: PathBuf,
    pub grants: Grants,
    pub inspectable: bool,
}

#[derive(Clone, Debug)]
pub struct LoadedExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub has_background: bool,
}

pub(crate) struct Loaded {
    pub(crate) context: Retained<WKWebExtensionContext>,
}

pub(crate) struct Shared {
    host: Rc<dyn Host>,
    pub(crate) graph: RefCell<Graph>,
    pub(crate) loaded: RefCell<HashMap<String, Loaded>>,
    pub(crate) mtm: MainThreadMarker,
}

impl Shared {
    pub(crate) fn host(&self) -> &Rc<dyn Host> {
        &self.host
    }

    pub(crate) fn log(&self, context: &WKWebExtensionContext, level: LogLevel, message: &str) {
        let id = unsafe { context.uniqueIdentifier() }.to_string();
        self.host.log(&id, level, message);
    }
}

/// One profile's extension runtime.
pub struct Runtime {
    controller: Retained<WKWebExtensionController>,
    _delegate: Retained<Delegate>,
    shared: Rc<Shared>,
}

impl Runtime {
    /// `identifier` names a persistent controller whose extension storage
    /// survives restarts; `None` keeps everything in memory.
    pub fn new(
        mtm: MainThreadMarker,
        store: &WKWebsiteDataStore,
        identifier: Option<&NSUUID>,
        host: Rc<dyn Host>,
    ) -> Self {
        static SCHEME_REGISTERED: Once = Once::new();
        SCHEME_REGISTERED.call_once(|| unsafe {
            WKWebExtensionMatchPattern::registerCustomURLScheme(&NSString::from_str(SCHEME), mtm)
        });

        let configuration = unsafe {
            match identifier {
                Some(identifier) => {
                    WKWebExtensionControllerConfiguration::configurationWithIdentifier(
                        identifier, mtm,
                    )
                }
                None => WKWebExtensionControllerConfiguration::nonPersistentConfiguration(mtm),
            }
        };
        let views = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe {
            views.setWebsiteDataStore(store);
            views.setApplicationNameForUserAgent(Some(&NSString::from_str(application_name())));
            configuration.setDefaultWebsiteDataStore(Some(store));
            configuration.setWebViewConfiguration(Some(&views));
        }
        let controller = unsafe {
            WKWebExtensionController::initWithConfiguration(
                WKWebExtensionController::alloc(mtm),
                &configuration,
            )
        };
        let shared = Rc::new(Shared {
            host,
            graph: RefCell::new(Graph::default()),
            loaded: RefCell::new(HashMap::new()),
            mtm,
        });
        let delegate = Delegate::new(mtm, Rc::downgrade(&shared));
        unsafe { controller.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        Self {
            controller,
            _delegate: delegate,
            shared,
        }
    }

    pub fn controller(&self) -> &WKWebExtensionController {
        &self.controller
    }

    /// Attaches the runtime to a tab's configuration before its view exists.
    pub fn configure(&self, configuration: &WKWebViewConfiguration) {
        unsafe {
            configuration.setWebExtensionController(Some(&self.controller));
            configuration
                .setApplicationNameForUserAgent(Some(&NSString::from_str(application_name())));
        }
    }

    pub fn context(&self, id: &str) -> Option<Retained<WKWebExtensionContext>> {
        self.shared
            .loaded
            .borrow()
            .get(id)
            .map(|loaded| loaded.context.clone())
    }

    pub fn loaded_ids(&self) -> Vec<String> {
        self.shared.loaded.borrow().keys().cloned().collect()
    }

    /// Loads a prepared package. WebKit reads the package asynchronously.
    pub fn load(
        &self,
        spec: ExtensionSpec,
        done: impl FnOnce(Result<LoadedExtension, String>) + 'static,
    ) {
        if self.shared.loaded.borrow().contains_key(&spec.id) {
            done(Err(format!("{} is already loaded", spec.id)));
            return;
        }
        let Some(url) = directory_url(&spec.root) else {
            done(Err(format!(
                "{} is not a readable directory",
                spec.root.display()
            )));
            return;
        };
        let shared = Rc::downgrade(&self.shared);
        let controller = self.controller.clone();
        let done = RefCell::new(Some(done));
        let spec = RefCell::new(Some(spec));
        let completion =
            RcBlock::new(move |extension: *mut WKWebExtension, error: *mut NSError| {
                let (Some(done), Some(spec)) = (done.take(), spec.take()) else {
                    return;
                };
                let Some(shared) = shared.upgrade() else {
                    return done(Err("the runtime was closed".to_owned()));
                };
                let result = match unsafe { extension.as_ref() } {
                    Some(extension) => activate(&shared, &controller, extension, spec),
                    None => Err(describe(unsafe { error.as_ref() })),
                };
                done(result);
            });
        unsafe {
            WKWebExtension::extensionWithResourceBaseURL_completionHandler(
                &url,
                &completion,
                self.shared.mtm,
            )
        };
    }

    pub fn unload(&self, id: &str) -> bool {
        let Some(loaded) = self.shared.loaded.borrow_mut().remove(id) else {
            return false;
        };
        unsafe {
            self.controller
                .unloadExtensionContext_error(&loaded.context)
        }
        .is_ok()
    }

    /// Starts an extension's background now rather than on its first event.
    pub fn start_background(&self, id: &str, done: impl FnOnce(Result<(), String>) + 'static) {
        let Some(context) = self.context(id) else {
            return done(Err(format!("{id} is not loaded")));
        };
        let done = RefCell::new(Some(done));
        let completion = RcBlock::new(move |error: *mut NSError| {
            if let Some(done) = done.take() {
                done(match unsafe { error.as_ref() } {
                    None => Ok(()),
                    Some(error) => Err(describe(Some(error))),
                });
            }
        });
        unsafe { context.loadBackgroundContentWithCompletionHandler(&completion) };
    }

    /// Runs an extension's toolbar action as a user click on `tab`: WebKit
    /// either fires `action.onClicked` or asks the host to present the popup.
    pub fn perform_action(&self, id: &str, tab: Option<u64>) -> bool {
        let Some(context) = self.context(id) else {
            return false;
        };
        let tab = tab.and_then(|tab| self.shared.graph.borrow().tab(tab));
        unsafe {
            if let Some(tab) = &tab {
                context.userGesturePerformedInTab(ProtocolObject::from_ref(&**tab));
            }
            context.performActionForTab(tab.as_deref().map(ProtocolObject::from_ref));
        }
        true
    }

    /// Replaces the browser's windows and tabs, telling WebKit what changed.
    pub fn publish(&self, windows: &[WindowSnapshot], focused: Option<u64>) {
        let mtm = self.shared.mtm;
        let (previous_windows, previous_tabs, previous_active, previous_focus) = {
            let graph = self.shared.graph.borrow();
            let tabs: HashMap<u64, (Retained<Tab>, u64, usize)> = graph
                .windows
                .iter()
                .flat_map(|window| {
                    window
                        .tabs()
                        .into_iter()
                        .enumerate()
                        .map(move |(index, tab)| (tab.id(), (tab, window.id(), index)))
                        .collect::<Vec<_>>()
                })
                .collect();
            let active: HashMap<u64, Option<u64>> = graph
                .windows
                .iter()
                .map(|window| (window.id(), window.active_id()))
                .collect();
            (graph.windows.clone(), tabs, active, graph.focused)
        };

        let mut windows_out = Vec::with_capacity(windows.len());
        let mut opened_windows = Vec::new();
        let mut opened_tabs = Vec::new();
        let mut changed = Vec::new();
        let mut moved = Vec::new();
        let mut activated = Vec::new();
        let mut kept_tabs = HashMap::new();
        for snapshot in windows {
            let window = previous_windows
                .iter()
                .find(|window| window.id() == snapshot.id)
                .cloned()
                .unwrap_or_else(|| {
                    let window = Window::new(mtm, snapshot.id, Rc::downgrade(&self.shared));
                    opened_windows.push(window.clone());
                    window
                });
            window.set_frame(snapshot.frame);
            let mut tabs = Vec::with_capacity(snapshot.tabs.len());
            for (index, tab_snapshot) in snapshot.tabs.iter().enumerate() {
                let tab = match previous_tabs.get(&tab_snapshot.id) {
                    Some((tab, window_id, old_index)) => {
                        if *window_id != snapshot.id || *old_index != index {
                            moved.push((tab.clone(), *old_index, *window_id));
                        }
                        tab.clone()
                    }
                    None => {
                        let tab = Tab::new(mtm, tab_snapshot.id, Rc::downgrade(&self.shared));
                        opened_tabs.push(tab.clone());
                        tab
                    }
                };
                let changes = tab.update(tab_snapshot.clone());
                if changes.any() && previous_tabs.contains_key(&tab_snapshot.id) {
                    changed.push((tab.clone(), changes));
                }
                kept_tabs.insert(tab_snapshot.id, ());
                tabs.push(tab);
            }
            let before = previous_active.get(&snapshot.id).copied().flatten();
            window.set_tabs(tabs, snapshot.active);
            if snapshot.active != before {
                activated.push((window.clone(), before));
            }
            windows_out.push(window);
        }
        let closed_tabs: Vec<_> = previous_tabs
            .iter()
            .filter(|(id, _)| !kept_tabs.contains_key(id))
            .map(|(_, (tab, window_id, _))| (tab.clone(), *window_id))
            .collect();
        let closed_windows: Vec<_> = previous_windows
            .iter()
            .filter(|window| windows.iter().all(|snapshot| snapshot.id != window.id()))
            .cloned()
            .collect();
        {
            let mut graph = self.shared.graph.borrow_mut();
            graph.windows = windows_out;
            graph.focused = focused;
        }

        let controller = &self.controller;
        unsafe {
            for window in &opened_windows {
                controller.didOpenWindow(ProtocolObject::from_ref(&**window));
            }
            for tab in &opened_tabs {
                controller.didOpenTab(ProtocolObject::from_ref(&**tab));
            }
            for (tab, window_id) in &closed_tabs {
                let window_closing = closed_windows
                    .iter()
                    .any(|window| window.id() == *window_id);
                controller
                    .didCloseTab_windowIsClosing(ProtocolObject::from_ref(&**tab), window_closing);
                tab.detach();
            }
            for window in &closed_windows {
                controller.didCloseWindow(ProtocolObject::from_ref(&**window));
            }
            for (tab, old_index, old_window) in &moved {
                let old_window = previous_windows
                    .iter()
                    .find(|window| window.id() == *old_window);
                controller.didMoveTab_fromIndex_inWindow(
                    ProtocolObject::from_ref(&**tab),
                    *old_index,
                    old_window.map(|window| ProtocolObject::from_ref(&**window)),
                );
            }
            for (tab, changes) in &changed {
                controller.didChangeTabProperties_forTab(
                    tab_properties(*changes),
                    ProtocolObject::from_ref(&**tab),
                );
            }
            for (window, before) in &activated {
                if let Some(active) = window.active_tab() {
                    let previous = before.and_then(|id| self.shared.graph.borrow().tab(id));
                    controller.didActivateTab_previousActiveTab(
                        ProtocolObject::from_ref(&*active),
                        previous.as_deref().map(ProtocolObject::from_ref),
                    );
                }
            }
            if focused != previous_focus {
                let window = focused.and_then(|id| self.shared.graph.borrow().window(id));
                controller.didFocusWindow(window.as_deref().map(ProtocolObject::from_ref));
            }
        }
    }

    /// Associates a tab with the web view currently showing it.
    pub fn bind_view(&self, tab: u64, view: Option<&WKWebView>) {
        if let Some(tab) = self.shared.graph.borrow().tab(tab) {
            tab.set_webview(view);
        }
    }

    pub fn tab_object(
        &self,
        tab: u64,
    ) -> Option<Retained<ProtocolObject<dyn objc2_web_kit::WKWebExtensionTab>>> {
        self.shared
            .graph
            .borrow()
            .tab(tab)
            .map(ProtocolObject::from_retained)
    }
}

fn activate(
    shared: &Rc<Shared>,
    controller: &WKWebExtensionController,
    extension: &WKWebExtension,
    spec: ExtensionSpec,
) -> Result<LoadedExtension, String> {
    let context = unsafe { WKWebExtensionContext::contextForExtension(extension) };
    let base = NSURL::URLWithString(&NSString::from_str(&format!("{SCHEME}://{}/", spec.id)))
        .ok_or_else(|| format!("{} is not a valid extension ID", spec.id))?;
    unsafe {
        context.setUniqueIdentifier(&NSString::from_str(&spec.id));
        context.setBaseURL(&base);
        context.setInspectable(spec.inspectable);
    }
    apply_grants(shared.mtm, &context, extension, &spec.grants);
    unsafe { controller.loadExtensionContext_error(&context) }
        .map_err(|error| describe(Some(&error)))?;

    let name = unsafe { extension.displayName() }
        .map(|name| name.to_string())
        .unwrap_or_else(|| spec.id.clone());
    let loaded = LoadedExtension {
        id: spec.id.clone(),
        name,
        version: unsafe { extension.version() }
            .map(|version| version.to_string())
            .unwrap_or_default(),
        has_background: unsafe { extension.hasBackgroundContent() },
    };
    for error in unsafe { context.errors() }.iter() {
        shared.log(&context, LogLevel::Warning, &describe(Some(&error)));
    }
    shared
        .loaded
        .borrow_mut()
        .insert(spec.id, Loaded { context });
    Ok(loaded)
}

fn apply_grants(
    mtm: MainThreadMarker,
    context: &WKWebExtensionContext,
    extension: &WKWebExtension,
    grants: &Grants,
) {
    let granted = WKWebExtensionContextPermissionStatus::GrantedExplicitly;
    unsafe {
        match grants {
            Grants::Requested => {
                for permission in extension.requestedPermissions().iter() {
                    context.setPermissionStatus_forPermission(granted, &permission);
                }
                for pattern in extension.allRequestedMatchPatterns().iter() {
                    context.setPermissionStatus_forMatchPattern(granted, &pattern);
                }
            }
            Grants::Explicit {
                permissions,
                match_patterns,
            } => {
                for permission in permissions {
                    context.setPermissionStatus_forPermission(
                        granted,
                        &NSString::from_str(permission),
                    );
                }
                for pattern in match_patterns {
                    if let Some(pattern) = WKWebExtensionMatchPattern::matchPatternWithString(
                        &NSString::from_str(pattern),
                        mtm,
                    ) {
                        context.setPermissionStatus_forMatchPattern(granted, &pattern);
                    }
                }
            }
        }
        // The internal bridge and native hosts are reached through native
        // messaging, which the browser authorizes itself.
        context.setPermissionStatus_forPermission(granted, &NSString::from_str("nativeMessaging"));
        // With a custom scheme registered, WebKit counts other extensions'
        // pages as part of <all_urls>; Chrome never does.
        for scheme in [SCHEME, "webkit-extension"] {
            if let Some(pattern) = WKWebExtensionMatchPattern::matchPatternWithString(
                &NSString::from_str(&format!("{scheme}://*/*")),
                mtm,
            ) {
                context.setPermissionStatus_forMatchPattern(
                    WKWebExtensionContextPermissionStatus::DeniedExplicitly,
                    &pattern,
                );
            }
        }
    }
}

fn tab_properties(
    changes: crate::surface::TabChanges,
) -> objc2_web_kit::WKWebExtensionTabChangedProperties {
    use objc2_web_kit::WKWebExtensionTabChangedProperties as Properties;
    let mut properties = Properties::empty();
    if changes.title {
        properties |= Properties::Title;
    }
    if changes.url {
        properties |= Properties::URL;
    }
    if changes.loading {
        properties |= Properties::Loading;
    }
    if changes.pinned {
        properties |= Properties::Pinned;
    }
    properties
}

fn directory_url(path: &std::path::Path) -> Option<Retained<NSURL>> {
    if !path.is_dir() {
        return None;
    }
    let path = NSString::from_str(path.to_str()?);
    Some(NSURL::fileURLWithPath_isDirectory(&path, true))
}

pub(crate) fn describe(error: Option<&NSError>) -> String {
    let Some(error) = error else {
        return "unknown error".to_owned();
    };
    // WebKit hands out WKNSError proxies that forward NSError's methods, which
    // objc2's debug-time method verification rejects; send them unchecked.
    unsafe fn text(receiver: &NSError, selector: objc2::runtime::Sel) -> Option<String> {
        let send: unsafe extern "C" fn(*const NSError, objc2::runtime::Sel) -> *const NSString = unsafe {
            std::mem::transmute(objc2::ffi::objc_msgSend as unsafe extern "C-unwind" fn())
        };
        let result = unsafe { send(receiver, selector) };
        unsafe { result.as_ref() }.map(|value| value.to_string())
    }
    let description = unsafe { text(error, objc2::sel!(localizedDescription)) };
    let reason = unsafe { text(error, objc2::sel!(localizedFailureReason)) };
    match (description, reason) {
        (Some(description), Some(reason)) => format!("{description}: {reason}"),
        (Some(description), None) => description,
        _ => unsafe { text(error, objc2::sel!(description)) }
            .unwrap_or_else(|| "unknown error".into()),
    }
}
