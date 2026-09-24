//! Main-thread native download coordinator. Native operations own transport;
//! the Store owns durable metadata; bounded workers own blocking filesystem IO.

mod lifecycle;
mod ui;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
use zephium_core::downloads::*;
use zephium_core::ids::{DownloadId, ProfileId};
use zephium_core::permissions::PageOrigin;
use zephium_core::ports::{engine::Partition, store::Store};

use super::download_files::{verify_file, Destination};
use super::file_uploads::PanelLease;
use super::permits::EventPermit;
use crate::navigation_epoch::{NavigationActivity, NavigationEpochTracker};

type SharedStore = Arc<dyn Store + Send + Sync>;
type Notify = Arc<dyn Fn(ProfileId) + Send + Sync>;
type DestinationReply = RcBlock<dyn Fn(*mut NSURL)>;
type DrainCompletion = Box<dyn FnOnce(bool) + Send>;
type DrainWaiter = (Option<ProfileId>, DrainCompletion);
const MAX_UI_CALLS: usize = 4;
const RECENT_LIMIT: usize = 64;

struct Source {
    permit: EventPermit,
    surface_intent: Arc<AtomicBool>,
    navigation: NavigationEpochTracker,
    activity: NavigationActivity,
    view: Retained<WKWebView>,
    window: Retained<NSWindow>,
}
impl Source {
    fn live(&self) -> bool {
        self.permit.active_token().is_some()
            && self.surface_intent.load(Ordering::Acquire)
            && self.navigation.matches_activity(self.activity)
            && unsafe { self.view.superview() }
                .is_some_and(|parent| !parent.isHiddenOrHasHiddenAncestor())
            && self.window.isVisible()
            && self
                .view
                .window()
                .is_some_and(|window| std::ptr::eq(&*window, &*self.window))
    }
}

struct Transfer {
    partition: Partition,
    record: DownloadRecord,
    native: Retained<WKDownload>,
    _delegate: Retained<DownloadDelegate>,
    source: Option<Source>,
    destination_reply: Option<DestinationReply>,
    destination: Option<Destination>,
    panel: Option<Retained<NSSavePanel>>,
    panel_lease: Option<PanelLease>,
    authorized: bool,
    cancelling: bool,
    persisting_terminal: bool,
    deadline: Instant,
}

enum Message {
    Preferences(DownloadId, DownloadStoreReply),
    Prepared(DownloadId, Result<Destination, DownloadError>),
    Persisted(DownloadId, u32, DownloadStoreReply),
    Finalized(DownloadId, Result<(PathBuf, FileIdentity), DownloadError>),
    Cancelled(DownloadId),
    Cleaned,
    Recovered(ProfileId, Vec<(DownloadId, FileIdentity)>),
    RecoverySaved(DownloadId, DownloadStoreReply),
    Ui(u64, DownloadStoreReply),
    Verified(u64, PathBuf, Result<(), DownloadError>),
    DirectorySelected(
        UiCall,
        DownloadPreferences,
        Result<(String, String), DownloadError>,
    ),
}
struct UiCall {
    partition: Partition,
    call: DownloadCall,
    done: DownloadCompletion,
}

pub(crate) struct Downloads {
    session: DownloadId,
    store: SharedStore,
    notify: Notify,
    active: RefCell<HashMap<DownloadId, Transfer>>,
    recent: RefCell<VecDeque<(Partition, DownloadRecord)>>,
    forgotten: RefCell<VecDeque<(ProfileId, DownloadId)>>,
    preferences: RefCell<HashMap<ProfileId, DownloadPreferences>>,
    calls: RefCell<HashMap<u64, UiCall>>,
    next_call: Cell<u64>,
    sender: mpsc::Sender<Message>,
    receiver: RefCell<mpsc::Receiver<Message>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
    stopping: Cell<bool>,
    retired: RefCell<HashSet<ProfileId>>,
    waiters: RefCell<Vec<DrainWaiter>>,
    persistence_failed: Cell<bool>,
    recovering: Cell<bool>,
    recovered: RefCell<VecDeque<DownloadId>>,
    work: Cell<usize>,
    directory_panels: RefCell<HashMap<u64, (Retained<NSOpenPanel>, PanelLease)>>,
}

impl Downloads {
    pub(crate) fn new(store: SharedStore, notify: Notify) -> Rc<Self> {
        let (sender, receiver) = mpsc::channel();
        Rc::new(Self {
            session: DownloadId::generate(),
            store,
            notify,
            active: RefCell::new(HashMap::new()),
            recent: RefCell::new(VecDeque::new()),
            forgotten: RefCell::new(VecDeque::new()),
            preferences: RefCell::new(HashMap::new()),
            calls: RefCell::new(HashMap::new()),
            next_call: Cell::new(1),
            sender,
            receiver: RefCell::new(receiver),
            timer: RefCell::new(None),
            stopping: Cell::new(false),
            retired: RefCell::new(HashSet::new()),
            waiters: RefCell::new(Vec::new()),
            persistence_failed: Cell::new(false),
            recovering: Cell::new(false),
            recovered: RefCell::new(VecDeque::new()),
            work: Cell::new(0),
            directory_panels: RefCell::new(HashMap::new()),
        })
    }

    fn ensure_timer(self: &Rc<Self>) {
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

    pub(super) fn admit(
        self: &Rc<Self>,
        partition: Partition,
        permit: EventPermit,
        surface_intent: Arc<AtomicBool>,
        navigation: NavigationEpochTracker,
        native: &WKDownload,
    ) {
        if self.stopping.get()
            || self.retired.borrow().contains(&partition.profile())
            || self.active.borrow().len() >= MAX_ACTIVE_DOWNLOADS
            || permit.active_token().is_none()
            || !surface_intent.load(Ordering::Acquire)
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
        let Some(window) = view.window() else {
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
            state: DownloadState::Pending,
            received: 0,
            total: None,
            error: None,
            destination: None,
            staging: None,
            staging_identity: None,
            identity: None,
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
                authorized: false,
                cancelling: false,
                persisting_terminal: false,
                deadline: Instant::now() + Duration::from_secs(30),
            },
        );
        unsafe { native.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        self.ensure_timer();
        (self.notify)(partition.profile());
    }

    fn destination(
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
            transfer.destination_reply = Some(completion.copy());
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

    fn choose_destination(self: &Rc<Self>, id: DownloadId, preferences: DownloadPreferences) {
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

    fn prepare(self: &Rc<Self>, id: DownloadId, path: PathBuf, expected_directory: Option<String>) {
        {
            let mut active = self.active.borrow_mut();
            let Some(transfer) = active.get_mut(&id) else {
                return;
            };
            if transfer.cancelling || !transfer.source.as_ref().is_some_and(Source::live) {
                drop(active);
                self.cancel(id, None);
                return;
            }
            transfer.authorized = true;
            transfer.source = None;
            transfer.deadline = Instant::now() + Duration::from_secs(30);
        }
        self.work.set(self.work.get() + 1);
        let sender = self.sender.clone();
        if std::thread::Builder::new()
            .name("zephium-download-destination".into())
            .spawn(move || {
                let result = Destination::prepare(id, path, expected_directory);
                let _ = sender.send(Message::Prepared(id, result));
            })
            .is_err()
        {
            self.work.set(self.work.get() - 1);
            self.cancel(id, Some(DownloadError::Unavailable));
        }
    }

    fn persist(&self, id: DownloadId) {
        let entry = self
            .active
            .borrow()
            .get(&id)
            .map(|transfer| (transfer.partition, transfer.record.clone()));
        let Some((partition, record)) = entry else {
            return;
        };
        let revision = record.revision;
        self.work.set(self.work.get() + 1);
        if matches!(partition, Partition::Ephemeral(_)) {
            let _ = self
                .sender
                .send(Message::Persisted(id, revision, DownloadStoreReply::Saved));
            return;
        }
        let sender = self.sender.clone();
        if !self.store.download_call(
            partition.profile(),
            DownloadStoreCall::Save(Box::new(record)),
            Box::new(move |reply| {
                let _ = sender.send(Message::Persisted(id, revision, reply));
            }),
        ) {
            let _ = self.sender.send(Message::Persisted(
                id,
                revision,
                DownloadStoreReply::Error(DownloadError::Storage),
            ));
        }
    }

    fn native_finished(self: &Rc<Self>, id: DownloadId) {
        let work = {
            let mut active = self.active.borrow_mut();
            let Some(transfer) = active.get_mut(&id) else {
                return;
            };
            if transfer.cancelling || transfer.record.state != DownloadState::Receiving {
                return;
            }
            transfer.record.state = DownloadState::Finalizing;
            transfer.record.revision += 1;
            transfer.destination.take().map(|destination| {
                (
                    destination,
                    transfer.record.source.clone(),
                    transfer.partition.profile(),
                )
            })
        };
        let Some((destination, source, profile)) = work else {
            self.cancel(id, Some(DownloadError::Destination));
            return;
        };
        (self.notify)(profile);
        self.work.set(self.work.get() + 1);
        let sender = self.sender.clone();
        if std::thread::Builder::new()
            .name("zephium-download-publish".into())
            .spawn(move || {
                let _ = sender.send(Message::Finalized(id, destination.finish(&source)));
            })
            .is_err()
        {
            self.work.set(self.work.get() - 1);
            self.terminal(id, DownloadState::Failed, Some(DownloadError::Unavailable));
        }
    }

    fn native_failed(&self, id: DownloadId) {
        let cancelling = self
            .active
            .borrow()
            .get(&id)
            .is_some_and(|transfer| transfer.cancelling);
        self.terminal(
            id,
            if cancelling {
                DownloadState::Cancelled
            } else {
                DownloadState::Failed
            },
            if cancelling {
                None
            } else {
                Some(DownloadError::Network)
            },
        );
    }

    fn cancel(&self, id: DownloadId, error: Option<DownloadError>) {
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
            transfer.record.error = error;
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
            reply.call((std::ptr::null_mut(),));
        }
        let sender = self.sender.clone();
        let cancelled = RcBlock::new(move |_resume: *mut NSData| {
            let _ = sender.send(Message::Cancelled(id));
        });
        unsafe {
            native.0.cancel(Some(&cancelled));
        }
    }

    fn terminal(&self, id: DownloadId, state: DownloadState, error: Option<DownloadError>) {
        let destination = {
            let mut active = self.active.borrow_mut();
            let Some(transfer) = active.get_mut(&id) else {
                return;
            };
            if transfer.persisting_terminal {
                return;
            }
            transfer.record.error = transfer.record.error.or(error);
            transfer.record.state =
                if state == DownloadState::Cancelled && transfer.record.error.is_some() {
                    DownloadState::Failed
                } else {
                    state
                };
            transfer.record.revision += 1;
            transfer.persisting_terminal = true;
            transfer.destination.take()
        };
        if let Some(destination) = destination {
            // A terminal native callback proves it no longer writes payload.
            self.cleanup(destination);
        }
        self.persist(id);
    }

    fn tick(self: &Rc<Self>) {
        for _ in 0..64 {
            let message = self.receiver.borrow().try_recv().ok();
            let Some(message) = message else { break };
            self.message(message);
        }
        let mut cancel = Vec::new();
        let mut changed = Vec::new();
        let samples: Vec<_> = self
            .active
            .borrow()
            .iter()
            .filter(|(_, transfer)| transfer.record.state == DownloadState::Receiving)
            .map(|(id, transfer)| (*id, transfer.native.clone()))
            .collect();
        for (id, native) in samples {
            let progress = native.progress();
            let received = progress.completedUnitCount().max(0) as u64;
            let total = u64::try_from(progress.totalUnitCount())
                .ok()
                .filter(|value| *value > 0);
            if let Some(transfer) = self.active.borrow_mut().get_mut(&id) {
                if transfer.record.received != received
                    || (total.is_some() && transfer.record.total != total)
                {
                    transfer.record.revision += 1;
                    transfer.record.received = received;
                    if total.is_some() {
                        transfer.record.total = total;
                    }
                    changed.push(transfer.partition.profile());
                }
            }
        }
        for (id, transfer) in self.active.borrow().iter() {
            if transfer.record.state == DownloadState::Pending
                && (Instant::now() > transfer.deadline
                    || (!transfer.authorized
                        && !transfer.source.as_ref().is_some_and(Source::live)))
            {
                cancel.push(*id);
            }
        }
        for id in cancel {
            self.cancel(id, Some(DownloadError::Unavailable));
        }
        changed.sort();
        changed.dedup();
        for profile in changed {
            (self.notify)(profile);
        }
        if self.work.get() == 0 {
            let mut ready = Vec::new();
            {
                let mut waiters = self.waiters.borrow_mut();
                let mut index = 0;
                while index < waiters.len() {
                    let profile = waiters[index].0;
                    let active = self.active.borrow().values().any(|transfer| {
                        profile.is_none_or(|profile| transfer.partition.profile() == profile)
                    });
                    let calls = self.calls.borrow().values().any(|call| {
                        profile.is_none_or(|profile| call.partition.profile() == profile)
                    });
                    if !active && !calls {
                        ready.push(waiters.swap_remove(index));
                    } else {
                        index += 1;
                    }
                }
            }
            for (profile, done) in ready {
                self.recent
                    .borrow_mut()
                    .retain(|(owner, _)| profile.is_some_and(|profile| owner.profile() != profile));
                self.preferences
                    .borrow_mut()
                    .retain(|owner, _| profile.is_some_and(|profile| *owner != profile));
                done(!self.persistence_failed.get());
            }
        }
        if self.active.borrow().is_empty() && self.calls.borrow().is_empty() && self.work.get() == 0
        {
            if let Some(timer) = self.timer.borrow_mut().take() {
                timer.invalidate();
            }
        }
    }

    fn cleanup(&self, destination: Destination) {
        self.work.set(self.work.get() + 1);
        let sender = self.sender.clone();
        if std::thread::Builder::new()
            .name("zephium-download-cleanup".into())
            .spawn(move || {
                drop(destination);
                let _ = sender.send(Message::Cleaned);
            })
            .is_err()
        {
            self.work.set(self.work.get() - 1);
        }
    }

    fn message(self: &Rc<Self>, message: Message) {
        if !matches!(message, Message::Cancelled(_)) {
            self.work.set(self.work.get().saturating_sub(1));
        }
        match message {
            Message::Recovered(profile, records) => {
                self.recovering.set(false);
                for (id, expected) in records {
                    self.recovered.borrow_mut().push_back(id);
                    if self.recovered.borrow().len() > 512 {
                        self.recovered.borrow_mut().pop_front();
                    }
                    for (_, record) in self
                        .recent
                        .borrow_mut()
                        .iter_mut()
                        .filter(|(_, record)| record.id == id)
                    {
                        record.staging = None;
                        record.staging_identity = None;
                    }
                    self.work.set(self.work.get() + 1);
                    let sender = self.sender.clone();
                    if !self.store.download_call(
                        profile,
                        DownloadStoreCall::ClearStaging { id, expected },
                        Box::new(move |reply| {
                            let _ = sender.send(Message::RecoverySaved(id, reply));
                        }),
                    ) {
                        let _ = self.sender.send(Message::RecoverySaved(
                            id,
                            DownloadStoreReply::Error(DownloadError::Storage),
                        ));
                    }
                }
            }
            Message::RecoverySaved(id, reply) => {
                if !matches!(reply, DownloadStoreReply::Saved) {
                    self.recovered.borrow_mut().retain(|old| *old != id);
                }
            }
            Message::Cleaned => {}
            Message::Preferences(id, DownloadStoreReply::Preferences(preferences)) => {
                self.choose_destination(id, preferences)
            }
            Message::Preferences(id, _) => self.cancel(id, Some(DownloadError::Storage)),
            Message::Prepared(id, result) => match result {
                Ok(destination) => {
                    let mut active = self.active.borrow_mut();
                    let Some(transfer) =
                        active.get_mut(&id).filter(|transfer| !transfer.cancelling)
                    else {
                        drop(active);
                        self.cleanup(destination);
                        return;
                    };
                    transfer.record.filename = destination
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "download".into());
                    transfer.record.destination = Some(destination.path.clone());
                    transfer.record.staging = Some(destination.staging_path.clone());
                    transfer.record.staging_identity = Some(destination.staging_identity.clone());
                    transfer.destination = Some(destination);
                    transfer.record.revision += 1;
                    drop(active);
                    self.persist(id);
                }
                Err(error) => self.cancel(id, Some(error)),
            },
            Message::Persisted(id, revision, reply) => {
                let terminal = self
                    .active
                    .borrow()
                    .get(&id)
                    .filter(|transfer| transfer.record.revision == revision)
                    .map(|transfer| transfer.persisting_terminal);
                let Some(terminal) = terminal else { return };
                let saved = matches!(reply, DownloadStoreReply::Saved);
                if terminal {
                    let transfer = self.active.borrow_mut().remove(&id);
                    if let Some(mut transfer) = transfer {
                        if !saved {
                            self.persistence_failed.set(true);
                            transfer.record.error = Some(DownloadError::Storage);
                        }
                        let profile = transfer.partition.profile();
                        let mut recent = self.recent.borrow_mut();
                        recent.push_front((transfer.partition, transfer.record));
                        while recent.len() > RECENT_LIMIT {
                            if let Some((Partition::Ephemeral(owner), expired)) = recent.pop_back()
                            {
                                let mut forgotten = self.forgotten.borrow_mut();
                                forgotten.push_front((owner, expired.id));
                                while forgotten.len() > RECENT_LIMIT {
                                    forgotten.pop_back();
                                }
                            }
                        }
                        drop(recent);
                        (self.notify)(profile);
                    }
                } else if saved {
                    let start = {
                        let mut active = self.active.borrow_mut();
                        active
                            .get_mut(&id)
                            .filter(|transfer| !transfer.cancelling)
                            .and_then(|transfer| {
                                let destination = transfer.destination.as_ref()?;
                                let path = destination.payload();
                                let reply = transfer.destination_reply.take()?;
                                transfer.record.state = DownloadState::Receiving;
                                transfer.record.revision += 1;
                                Some((reply, path, transfer.partition.profile()))
                            })
                    };
                    if let Some((reply, path, profile)) = start {
                        if let Some(path) = path.to_str() {
                            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
                            reply.call((Retained::as_ptr(&url).cast_mut(),));
                            (self.notify)(profile);
                        } else {
                            reply.call((std::ptr::null_mut(),));
                            self.cancel(id, Some(DownloadError::Destination));
                        }
                    }
                } else {
                    self.cancel(id, Some(DownloadError::Storage));
                }
            }
            Message::Finalized(id, result) => match result {
                Ok((path, identity)) => {
                    if let Some(transfer) = self.active.borrow_mut().get_mut(&id) {
                        transfer.record.filename = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "download".into());
                        transfer.record.destination = Some(path);
                        transfer.record.received = identity.bytes;
                        transfer.record.total = Some(identity.bytes);
                        transfer.record.identity = Some(identity);
                        transfer.record.staging = None;
                        transfer.record.staging_identity = None;
                    }
                    self.terminal(id, DownloadState::Completed, None);
                }
                Err(error) => self.terminal(id, DownloadState::Failed, Some(error)),
            },
            Message::Cancelled(id) => self.terminal(id, DownloadState::Cancelled, None),
            Message::Ui(token, reply) => self.ui_reply(token, reply),
            Message::Verified(token, path, result) => self.verified(token, path, result),
            Message::DirectorySelected(request, mut preferences, result) => {
                if self.stopping.get()
                    || self.retired.borrow().contains(&request.partition.profile())
                {
                    return;
                }
                match result {
                    Ok((path, identity)) => {
                        preferences.directory = Some(path);
                        preferences.directory_identity = Some(identity);
                        self.finish_directory_selection(request, preferences);
                    }
                    Err(error) => request.done.finish(DownloadResponse::Error { error }),
                }
            }
        }
    }
}

pub(super) fn bounded(value: &NSString, max: usize) -> Option<String> {
    if value.length() > max {
        return None;
    }
    let value = value.to_string();
    (value.len() <= max).then_some(value)
}

struct DelegateIvars {
    manager: Weak<Downloads>,
    // Objective-C ivars on macOS cannot carry Rust's 16-byte u128 alignment.
    // Keep the ULID behind an ordinary pointer-aligned owner.
    id: Box<DownloadId>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind=MainThreadOnly]
    #[ivars=DelegateIvars]
    struct DownloadDelegate;
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
