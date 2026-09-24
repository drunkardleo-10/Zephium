//! WebKit/AppKit download transport and UI; shared state/recovery stays in the coordinator.
use super::super::file_uploads::PanelLease;
use super::*;
use block2::{Block, RcBlock};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::ProtocolObject, DefinedClass, MainThreadOnly,
};
use objc2_app_kit::{
    NSModalResponse, NSModalResponseOK, NSOpenPanel, NSSavePanel, NSWindow, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSData, NSError, NSHTTPURLResponse, NSObject, NSObjectProtocol,
    NSProgressReporting, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer,
    NSURLAuthenticationChallenge, NSURLAuthenticationMethodServerTrust, NSURLCredential,
    NSURLRequest, NSURLResponse, NSURLSessionAuthChallengeDisposition, NSURL,
};
use objc2_web_kit::{WKDownload, WKDownloadDelegate, WKDownloadRedirectPolicy, WKWebView};

pub(super) type Native = Retained<WKDownload>;
pub(super) type Delegate = Retained<DownloadDelegate>;
pub(super) type Timer = Retained<NSTimer>;
pub(super) type SavePanel = Retained<NSSavePanel>;
pub(super) type DirectoryPanel = Retained<NSOpenPanel>;
pub(super) type DialogLease = PanelLease;

pub(super) fn progress(native: &Native) -> Option<(u64, Option<u64>)> {
    let progress = native.progress();
    Some((
        progress.completedUnitCount().max(0) as u64,
        u64::try_from(progress.totalUnitCount())
            .ok()
            .filter(|value| *value > 0),
    ))
}
pub(super) fn stop_timer(timer: Timer) {
    timer.invalidate();
}
pub(super) fn cancel_directory(panel: &DirectoryPanel) {
    unsafe { panel.cancel(None) };
    panel.orderOut(None);
}
pub(super) fn reveal(path: &std::path::Path) -> Result<(), DownloadError> {
    let path = path.to_str().ok_or(DownloadError::Destination)?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    Ok(())
}
pub(super) fn open(path: &std::path::Path) -> Result<(), DownloadError> {
    let path = path.to_str().ok_or(DownloadError::Destination)?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err(DownloadError::Unavailable)
    }
}

pub(super) struct Source {
    permit: EventPermit,
    surface_intent: Arc<AtomicBool>,
    navigation: NavigationEpochTracker,
    activity: NavigationActivity,
    view: Retained<WKWebView>,
    window: Retained<NSWindow>,
    initial_open: bool,
}
impl Source {
    pub(super) fn live(&self) -> bool {
        self.permit.active_token().is_some()
            && (self.initial_open || self.surface_intent.load(Ordering::Acquire))
            && self.navigation.matches_activity(self.activity)
            && (self.initial_open
                || unsafe { self.view.superview() }
                    .is_some_and(|parent| !parent.isHiddenOrHasHiddenAncestor()))
            && self.window.isVisible()
            && (self.initial_open
                || self
                    .view
                    .window()
                    .is_some_and(|window| std::ptr::eq(&*window, &*self.window)))
    }
}

impl Downloads {
    pub(super) fn ensure_timer(self: &Rc<Self>) {
        if self.timer.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let callback = RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| {
            if let Some(manager) = weak.upgrade() {
                manager.tick();
            }
        });
        let timer =
            unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.2, true, &callback) };
        unsafe {
            NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
        }
        *self.timer.borrow_mut() = Some(timer);
    }
    pub(in crate::host) fn admit(
        self: &Rc<Self>,
        partition: Partition,
        permit: EventPermit,
        surface_intent: Arc<AtomicBool>,
        navigation: NavigationEpochTracker,
        native: &WKDownload,
        mut initial: Option<InitialDownload>,
    ) {
        if self.stopping.get()
            || self.retired.borrow().contains(&partition.profile())
            || self.active.borrow().len() >= MAX_ACTIVE_DOWNLOADS
            || self.work.get() >= MAX_BACKGROUND_WORK
            || !self.private_cleanup_capacity(partition)
            || permit.active_token().is_none()
            || (initial.is_none() && !surface_intent.load(Ordering::Acquire))
        {
            unsafe { native.cancel(None) };
            return;
        }
        let request_url = unsafe { native.originalRequest().and_then(|request| request.URL()) };
        let scheme = request_url
            .as_ref()
            .and_then(|url| url.scheme())
            .and_then(|scheme| bounded(&scheme, 16));
        let network_url = if matches!(scheme.as_deref(), Some("http" | "https")) {
            request_url
                .as_ref()
                .and_then(|url| url.absoluteString())
                .and_then(|url| bounded(&url, 8192))
                .filter(|url| zephium_core::navigation::is_allowed_str(url))
                .and_then(|url| url::Url::parse(&url).ok())
        } else {
            None
        };
        if network_url.is_none() && !matches!(scheme.as_deref(), Some("blob" | "data")) {
            unsafe { native.cancel(None) };
            return;
        }
        let Some(view) = (unsafe { native.webView() }) else {
            unsafe { native.cancel(None) };
            return;
        };
        let initial_open = initial.is_some();
        let Some(window) = initial
            .as_ref()
            .map(|context| context.window.clone())
            .or_else(|| view.window())
        else {
            unsafe { native.cancel(None) };
            return;
        };
        let Some(activity) = navigation.activity_snapshot() else {
            unsafe { native.cancel(None) };
            return;
        };
        let source = Source {
            navigation,
            activity,
            permit,
            surface_intent,
            view,
            window,
            initial_open,
        };
        if !source.live() || !source.window.isKeyWindow() {
            unsafe { native.cancel(None) };
            return;
        }
        let origin = unsafe {
            if native.respondsToSelector(objc2::sel!(originatingFrame)) {
                let native_origin = native.originatingFrame().securityOrigin();
                let scheme = bounded(&native_origin.protocol(), 16);
                let host = bounded(&native_origin.host(), 512);
                scheme.zip(host).and_then(|(scheme, host)| {
                    let port = native_origin.port();
                    if !(0..=65535).contains(&port) {
                        return None;
                    }
                    PageOrigin::from_native_components(
                        &scheme,
                        &host,
                        (port != 0).then_some(port as u16),
                    )
                    .ok()
                })
            } else {
                source
                    .view
                    .URL()
                    .and_then(|url| url.absoluteString())
                    .and_then(|url| bounded(&url, 8192))
                    .and_then(|url| url::Url::parse(&url).ok())
                    .and_then(|url| PageOrigin::from_url(&url).ok())
            }
        };
        let origin = origin.or_else(|| {
            network_url
                .as_ref()
                .and_then(|url| PageOrigin::from_url(url).ok())
        });
        let Some(origin) = origin else {
            unsafe { native.cancel(None) };
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            unsafe { native.cancel(None) };
            return;
        };
        let id = DownloadId::generate();
        let delegate = DownloadDelegate::new(mtm, Rc::downgrade(self), id);
        let record = DownloadRecord {
            id,
            session: self.session,
            revision: 1,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| time.as_secs() as i64),
            filename: "download".into(),
            source: origin.as_str().into(),
            source_is_context: false,
            state: DownloadState::Pending,
            received: 0,
            total: None,
            error: None,
            destination: None,
            staging: None,
            staging_identity: None,
            identity: None,
            writer: None,
            writer_released: false,
        };
        self.active.borrow_mut().insert(
            id,
            Transfer {
                partition,
                record,
                native: native.into(),
                _delegate: delegate.clone(),
                source: Some(source),
                destination_reply: None,
                destination: None,
                panel: None,
                panel_lease: None,
                on_started: initial
                    .as_mut()
                    .and_then(|context| context.on_started.take()),
                authorized: false,
                cancelling: false,
                persisting_terminal: false,
                deadline: Instant::now() + Duration::from_secs(30),
            },
        );
        unsafe { native.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        self.ensure_recovery(partition);
        self.ensure_timer();
        (self.notify)(partition.profile());
    }
    pub(super) fn destination(
        self: &Rc<Self>,
        id: DownloadId,
        response: &NSURLResponse,
        suggested: &NSString,
        completion: &Block<dyn Fn(*mut NSURL)>,
    ) {
        let name = bounded(suggested, 4096)
            .map(|value| safe_filename(&value))
            .unwrap_or_else(|| "download".into());
        let accepts = self.active.borrow().get(&id).is_some_and(|transfer| {
            transfer.record.state == DownloadState::Pending
                && transfer.destination_reply.is_none()
                && transfer.destination.is_none()
                && !transfer.cancelling
        });
        if !accepts {
            completion.call((std::ptr::null_mut(),));
            return;
        }
        let partition = {
            let mut active = self.active.borrow_mut();
            let Some(transfer) = active.get_mut(&id) else {
                return;
            };
            transfer.record.filename = name;
            transfer.record.total = u64::try_from(response.expectedContentLength()).ok();
            let completion = completion.copy();
            transfer.destination_reply = Some(DestinationReply::new(move |path| {
                let url = path.and_then(|path| {
                    path.to_str()
                        .map(|path| NSURL::fileURLWithPath(&NSString::from_str(path)))
                });
                completion.call((url
                    .as_ref()
                    .map_or(std::ptr::null_mut(), |url| Retained::as_ptr(url).cast_mut()),));
            }));
            transfer.partition
        };
        if let Some(preferences) = self.preferences.borrow().get(&partition.profile()).cloned() {
            self.choose_destination(id, preferences);
        } else if matches!(partition, Partition::Ephemeral(_)) {
            self.choose_destination(id, DownloadPreferences::default());
        } else {
            self.work.set(self.work.get() + 1);
            let sender = self.sender.clone();
            if !self.store.download_call(
                partition.profile(),
                DownloadStoreCall::Preferences,
                Box::new(move |reply| {
                    let _ = sender.send(Message::Preferences(id, reply));
                }),
            ) {
                let _ = self.sender.send(Message::Preferences(
                    id,
                    DownloadStoreReply::Error(DownloadError::Storage),
                ));
            }
        }
    }
    pub(super) fn choose_destination(
        self: &Rc<Self>,
        id: DownloadId,
        preferences: DownloadPreferences,
    ) {
        let context = {
            let active = self.active.borrow();
            active.get(&id).and_then(|transfer| {
                transfer.source.as_ref().map(|source| {
                    (
                        source.live(),
                        source.window.clone(),
                        transfer.record.filename.clone(),
                        transfer.record.source.clone(),
                        unsafe {
                            transfer
                                .native
                                .respondsToSelector(objc2::sel!(isUserInitiated))
                                && transfer.native.isUserInitiated()
                        },
                    )
                })
            })
        };
        let Some((true, window, filename, source, user_initiated)) = context else {
            self.cancel(id, None);
            return;
        };
        let directory = preferences
            .directory
            .clone()
            .map(PathBuf::from)
            .or_else(dirs::download_dir);
        if !preferences.ask_destination
            && user_initiated
            && (preferences.directory.is_none() || preferences.directory_identity.is_some())
        {
            let Some(directory) = directory else {
                self.cancel(id, Some(DownloadError::Destination));
                return;
            };
            self.prepare(
                id,
                directory.join(filename),
                preferences.directory_identity.clone(),
            );
            return;
        }
        if window.attachedSheet().is_some() || !window.isKeyWindow() {
            self.cancel(id, Some(DownloadError::Capacity));
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            self.cancel(id, Some(DownloadError::Unavailable));
            return;
        };
        let Some(lease) = PanelLease::acquire() else {
            self.cancel(id, Some(DownloadError::Capacity));
            return;
        };
        let panel = NSSavePanel::savePanel(mtm);
        panel.setNameFieldStringValue(&NSString::from_str(&filename));
        panel.setMessage(Some(&NSString::from_str(&format!(
            "Save download from {source}"
        ))));
        if let Some(path) = directory.and_then(|path| path.to_str().map(str::to_owned)) {
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&path))));
        }
        let live = self.active.borrow().get(&id).is_some_and(|transfer| {
            !transfer.cancelling && transfer.source.as_ref().is_some_and(Source::live)
        });
        if !live || !window.isKeyWindow() || window.attachedSheet().is_some() {
            self.cancel(id, Some(DownloadError::Unavailable));
            return;
        }
        if let Some(transfer) = self.active.borrow_mut().get_mut(&id) {
            transfer.panel = Some(panel.clone());
            transfer.panel_lease = Some(lease);
            // A person may browse the filesystem for longer than a network
            // callback deadline. Lifetime revocation still applies each tick.
            transfer.deadline = Instant::now() + Duration::from_secs(24 * 60 * 60);
        }
        let weak = Rc::downgrade(self);
        let selected_panel = panel.clone();
        let callback = RcBlock::new(move |result: NSModalResponse| {
            let Some(manager) = weak.upgrade() else {
                return;
            };
            let path = (result == NSModalResponseOK)
                .then(|| selected_panel.URL())
                .flatten()
                .and_then(|url| url.path())
                .and_then(|path| bounded(&path, 4096))
                .map(PathBuf::from);
            selected_panel.orderOut(None);
            if let Some(transfer) = manager.active.borrow_mut().get_mut(&id) {
                transfer.panel = None;
                transfer.panel_lease.take();
            }
            match path {
                Some(path) => manager.prepare(id, path, None),
                None => manager.cancel(id, None),
            }
        });
        panel.beginSheetModalForWindow_completionHandler(&window, &callback);
    }
    pub(super) fn cancel(&self, id: DownloadId, error: Option<DownloadError>) {
        let native = {
            let mut active = self.active.borrow_mut();
            let Some(transfer) = active.get_mut(&id) else {
                return;
            };
            if transfer.record.state.terminal()
                || transfer.record.state == DownloadState::Finalizing
                || transfer.cancelling
            {
                return;
            }
            transfer.cancelling = true;
            transfer.record.state = DownloadState::Cancelling;
            transfer.record.revision += 1;
            transfer.record.error = error;
            (self.notify)(transfer.partition.profile());
            (
                transfer.native.clone(),
                transfer.destination_reply.take(),
                transfer.panel.take(),
            )
        };
        if let Some(panel) = native.2 {
            unsafe { panel.cancel(None) };
            panel.orderOut(None);
        }
        if let Some(transfer) = self.active.borrow_mut().get_mut(&id) {
            transfer.panel_lease.take();
        }
        if let Some(reply) = native.1 {
            reply.finish(None);
        }
        let sender = self.sender.clone();
        let cancelled = RcBlock::new(move |_resume: *mut NSData| {
            let _ = sender.send(Message::Cancelled(id));
        });
        unsafe {
            native.0.cancel(Some(&cancelled));
        }
    }
    pub(super) fn choose_directory(
        self: &Rc<Self>,
        token: u64,
        partition: Partition,
        preferences: DownloadPreferences,
        done: DownloadCompletion,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(window) = objc2_app_kit::NSApplication::sharedApplication(mtm).keyWindow() else {
            return;
        };
        if window.attachedSheet().is_some() {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Capacity,
            });
            return;
        }
        let Some(lease) = PanelLease::acquire() else {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Capacity,
            });
            return;
        };
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(true);
        panel.setCanChooseFiles(false);
        panel.setAllowsMultipleSelection(false);
        panel.setMessage(Some(&NSString::from_str(
            "Choose where Zephium saves downloads",
        )));
        if self.stopping.get()
            || self.retired.borrow().contains(&partition.profile())
            || !window.isVisible()
            || !window.isKeyWindow()
            || window.attachedSheet().is_some()
        {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Unavailable,
            });
            return;
        }
        self.calls.borrow_mut().insert(
            token,
            UiCall {
                partition,
                call: DownloadCall::ChooseDirectory,
                done,
            },
        );
        self.directory_panels
            .borrow_mut()
            .insert(token, (panel.clone(), lease));
        let selected = panel.clone();
        let weak = Rc::downgrade(self);
        let callback = RcBlock::new(move |result: NSModalResponse| {
            let Some(manager) = weak.upgrade() else {
                return;
            };
            let _panel_owner = manager.directory_panels.borrow_mut().remove(&token);
            let request = manager.calls.borrow_mut().remove(&token);
            let Some(request) = request else { return };
            let path = (result == NSModalResponseOK)
                .then(|| selected.URLs())
                .and_then(|urls| urls.firstObject())
                .and_then(|url| url.path())
                .and_then(|path| bounded(&path, 4096));
            selected.orderOut(None);
            if let Some(path) = path {
                manager.work.set(manager.work.get() + 1);
                let sender = manager.sender.clone();
                let preferences = preferences.clone();
                if std::thread::Builder::new()
                    .name("zephium-download-directory".into())
                    .spawn(move || {
                        let result =
                            super::super::download_files::select_directory(PathBuf::from(path));
                        let _ =
                            sender.send(Message::DirectorySelected(request, preferences, result));
                    })
                    .is_err()
                {
                    manager.work.set(manager.work.get() - 1);
                }
            } else {
                request.done.finish(DownloadResponse::Error {
                    error: DownloadError::Cancelled,
                });
            }
        });
        panel.beginSheetModalForWindow_completionHandler(&window, &callback);
    }
}
pub(super) fn bounded(value: &NSString, max: usize) -> Option<String> {
    if value.length() > max {
        return None;
    }
    let value = value.to_string();
    (value.len() <= max).then_some(value)
}

pub(super) struct DelegateIvars {
    manager: Weak<Downloads>,
    // Objective-C ivars on macOS cannot carry Rust's 16-byte u128 alignment.
    // Keep the ULID behind an ordinary pointer-aligned owner.
    id: Box<DownloadId>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind=MainThreadOnly]
    #[ivars=DelegateIvars]
    pub(super) struct DownloadDelegate;
    unsafe impl NSObjectProtocol for DownloadDelegate {}
    unsafe impl WKDownloadDelegate for DownloadDelegate {
        #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
        unsafe fn decide(
            &self,
            _download: &WKDownload,
            response: &NSURLResponse,
            name: &NSString,
            completion: &Block<dyn Fn(*mut NSURL)>,
        ) {
            if let Some(manager) = self.ivars().manager.upgrade() {
                manager.destination(*self.ivars().id, response, name, completion);
            } else {
                completion.call((std::ptr::null_mut(),));
            }
        }
        #[unsafe(method(downloadDidFinish:))]
        unsafe fn finished(&self, _download: &WKDownload) {
            if let Some(manager) = self.ivars().manager.upgrade() {
                manager.native_finished(*self.ivars().id);
            }
        }
        #[unsafe(method(download:didFailWithError:resumeData:))]
        unsafe fn failed(
            &self,
            _download: &WKDownload,
            _error: &NSError,
            _resume: Option<&NSData>,
        ) {
            if let Some(manager) = self.ivars().manager.upgrade() {
                manager.native_failed(*self.ivars().id);
            }
        }
        #[unsafe(method(download:willPerformHTTPRedirection:newRequest:decisionHandler:))]
        unsafe fn redirect(
            &self,
            _download: &WKDownload,
            _response: &NSHTTPURLResponse,
            request: &NSURLRequest,
            completion: &Block<dyn Fn(WKDownloadRedirectPolicy)>,
        ) {
            let allowed = request
                .URL()
                .and_then(|url| url.absoluteString())
                .and_then(|url| bounded(&url, 8192))
                .is_some_and(|url| zephium_core::navigation::is_allowed_str(&url));
            completion.call((if allowed {
                WKDownloadRedirectPolicy::Allow
            } else {
                WKDownloadRedirectPolicy::Cancel
            },));
        }
        #[unsafe(method(download:didReceiveAuthenticationChallenge:completionHandler:))]
        unsafe fn authentication(
            &self,
            _download: &WKDownload,
            challenge: &NSURLAuthenticationChallenge,
            completion: &Block<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
        ) {
            let trust = challenge
                .protectionSpace()
                .authenticationMethod()
                .isEqualToString(NSURLAuthenticationMethodServerTrust);
            completion.call((
                if trust {
                    NSURLSessionAuthChallengeDisposition::PerformDefaultHandling
                } else {
                    NSURLSessionAuthChallengeDisposition::CancelAuthenticationChallenge
                },
                std::ptr::null_mut(),
            ));
        }
    }
);
impl DownloadDelegate {
    fn new(mtm: MainThreadMarker, manager: Weak<Downloads>, id: DownloadId) -> Retained<Self> {
        let delegate = mtm.alloc::<Self>().set_ivars(DelegateIvars {
            manager,
            id: Box::new(id),
        });
        unsafe { msg_send![super(delegate), init] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn objc_delegate_ivars_fit_the_native_alignment_limit() {
        assert!(std::mem::align_of::<DelegateIvars>() <= std::mem::align_of::<usize>());
    }
}
