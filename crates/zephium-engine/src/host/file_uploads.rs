//! Human file selection for foreground macOS content views.
//!
//! The native responder is never sent to Shell/IPC. The original WebKit input
//! owns the selected URLs, while this broker owns the panel lifetime and the
//! exact physical-view/document authorization. Browser chrome and agent views
//! do not install this opt-in handler.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel, NSWindow};
use objc2_foundation::{MainThreadMarker, NSString};
use objc2_web_kit::WKWebView;
use wry::{FileUploadRequest, FileUploadResponder};
use zephium_core::permissions::PageOrigin;

use super::permits::EventPermit;
use crate::navigation_epoch::{NavigationActivity, NavigationEpoch, NavigationEpochTracker};

const MAX_SELECTED_ENTRIES: usize = 1024;

thread_local! {
    // No queue of page-owned requests, and no allocation/polling while idle.
    static PANEL_BUSY: Cell<bool> = const { Cell::new(false) };
}

pub(super) struct PanelLease;

impl PanelLease {
    pub(super) fn acquire() -> Option<Self> {
        PANEL_BUSY.with(|busy| (!busy.replace(true)).then(|| Self))
    }
}

impl Drop for PanelLease {
    fn drop(&mut self) {
        PANEL_BUSY.with(|busy| busy.set(false));
    }
}

struct PendingUpload {
    epoch: NavigationEpoch,
    activity: NavigationActivity,
    panel: Retained<NSOpenPanel>,
    view: Retained<WKWebView>,
    window: Retained<NSWindow>,
    multiple: bool,
    responder: FileUploadResponder,
    _lease: PanelLease,
}

pub(super) struct FileUploadBroker {
    permit: EventPermit,
    navigation: NavigationEpochTracker,
    presentation: Arc<AtomicBool>,
    pending: RefCell<Option<(Rc<()>, PendingUpload)>>,
}

impl FileUploadBroker {
    pub(super) fn new(
        permit: EventPermit,
        navigation: NavigationEpochTracker,
        presentation: Arc<AtomicBool>,
    ) -> Rc<Self> {
        Rc::new(Self {
            permit,
            navigation,
            presentation,
            pending: RefCell::new(None),
        })
    }

    fn is_current(&self, epoch: NavigationEpoch) -> bool {
        self.permit.active_token().is_some()
            && self.navigation.is_current(epoch)
            && self.presentation.load(Ordering::Acquire)
    }

    pub(super) fn present(
        self: &Rc<Self>,
        view: &WKWebView,
        request: FileUploadRequest,
        responder: FileUploadResponder,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(epoch) = self.navigation.current_committed() else {
            return;
        };
        if !self.is_current(epoch) || view.isHiddenOrHasHiddenAncestor() {
            return;
        }
        let Some(window) = view.window() else { return };
        if !window.isKeyWindow() || !window.isVisible() || window.attachedSheet().is_some() {
            return;
        }
        let Ok(origin) = PageOrigin::from_native_components(
            request.origin.scheme(),
            request.origin.host(),
            request.origin.port(),
        ) else {
            return;
        };
        let Some(activity) = self.navigation.activity_snapshot() else {
            return;
        };
        let Some(lease) = PanelLease::acquire() else {
            return;
        };
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(request.allows_directories);
        panel.setCanChooseFiles(!request.allows_directories);
        panel.setAllowsMultipleSelection(request.allows_multiple_selection);
        panel.setCanCreateDirectories(false);
        panel.setMessage(Some(&NSString::from_str(&format!(
            "Select {} to share with {}",
            if request.allows_directories {
                "folders"
            } else {
                "files"
            },
            origin.as_str(),
        ))));
        // AppKit calls can reenter. Revalidate after configuring native UI and
        // retain no borrow across presenting/dismissing a sheet or completion.
        if !self.is_current(epoch)
            || !self.navigation.matches_activity(activity)
            || view.isHiddenOrHasHiddenAncestor()
            || !window.isKeyWindow()
            || window.attachedSheet().is_some()
        {
            return;
        }
        let identity = Rc::new(());
        let pending = PendingUpload {
            epoch,
            activity,
            panel: panel.clone(),
            view: view.into(),
            window: window.clone(),
            multiple: request.allows_multiple_selection,
            responder,
            _lease: lease,
        };
        *self.pending.borrow_mut() = Some((identity.clone(), pending));
        let weak = Rc::downgrade(self);
        let completion = RcBlock::new(move |response: NSModalResponse| {
            if let Some(broker) = weak.upgrade() {
                broker.finish(&identity, response);
            }
        });
        panel.beginSheetModalForWindow_completionHandler(&window, &completion);
    }

    fn finish(&self, identity: &Rc<()>, response: NSModalResponse) {
        let Some(pending) = take_matching(&mut self.pending.borrow_mut(), identity) else {
            return;
        };
        let selected = response == NSModalResponseOK && self.can_complete(&pending);
        let urls = selected.then(|| pending.panel.URLs());
        let urls = urls.as_deref().filter(|urls| {
            valid_selection_count(urls.len(), pending.multiple)
                && urls.iter().all(|url| url.isFileURL())
        });
        // NSOpenPanel may still be on screen when its completion runs.
        pending.panel.orderOut(None);
        // Recheck after the native call above before returning file authority.
        let urls = urls.filter(|_| self.can_complete(&pending));
        pending.responder.respond(urls);
    }

    fn can_complete(&self, pending: &PendingUpload) -> bool {
        pending.window.isVisible()
            && !pending.view.isHiddenOrHasHiddenAncestor()
            && pending
                .view
                .window()
                .is_some_and(|window| std::ptr::eq(&*window, &*pending.window))
            && self.is_current(pending.epoch)
            && self.navigation.matches_activity(pending.activity)
    }

    pub(super) fn cancel(&self) {
        let pending = self.pending.borrow_mut().take();
        if let Some((_, pending)) = pending {
            cancel_pending(pending);
        }
    }
}

impl Drop for FileUploadBroker {
    fn drop(&mut self) {
        if let Some((_, pending)) = self.pending.get_mut().take() {
            cancel_pending(pending);
        }
    }
}

fn cancel_pending(pending: PendingUpload) {
    // The slot is empty before AppKit can invoke the stale panel callback.
    unsafe { pending.panel.cancel(None) };
    pending.panel.orderOut(None);
    pending.responder.respond(None);
}

fn take_matching<T>(pending: &mut Option<(Rc<()>, T)>, identity: &Rc<()>) -> Option<T> {
    if pending
        .as_ref()
        .is_some_and(|(current, _)| Rc::ptr_eq(current, identity))
    {
        pending.take().map(|(_, value)| value)
    } else {
        None
    }
}

fn valid_selection_count(count: usize, multiple: bool) -> bool {
    count > 0 && count <= if multiple { MAX_SELECTED_ENTRIES } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_capacity_is_exclusive_and_released_on_cancellation() {
        let lease = PanelLease::acquire().unwrap();
        assert!(PanelLease::acquire().is_none());
        assert!(PanelLease::acquire().is_none());
        drop(lease);
        assert!(PanelLease::acquire().is_some());
    }

    #[test]
    fn late_panel_callback_cannot_complete_a_successor_request() {
        let old = Rc::new(());
        let current = Rc::new(());
        let mut slot = Some((current.clone(), 42));
        assert_eq!(take_matching(&mut slot, &old), None);
        assert_eq!(take_matching(&mut slot, &current), Some(42));
        assert_eq!(take_matching(&mut slot, &current), None);
    }

    #[test]
    fn stale_view_navigation_and_presentation_cannot_return_files() {
        use wry::{NavigationEvent, NavigationEventPhase, NavigationId};
        let token = Arc::new(AtomicBool::new(true));
        let permit = EventPermit::bound(&token);
        let navigation = NavigationEpochTracker::new();
        let epoch = navigation.begin("https://example.com/").unwrap();
        for phase in [
            NavigationEventPhase::Started,
            NavigationEventPhase::Committed,
        ] {
            navigation.observe_navigation(&NavigationEvent {
                id: NavigationId::from_raw(1),
                phase,
                url: "https://example.com/".into(),
            });
        }
        let presentation = Arc::new(AtomicBool::new(true));
        let broker =
            FileUploadBroker::new(permit.clone(), navigation.clone(), presentation.clone());
        assert!(broker.is_current(epoch));
        presentation.store(false, Ordering::Release);
        assert!(!broker.is_current(epoch));
        presentation.store(true, Ordering::Release);
        let activity = navigation.activity_snapshot().unwrap();
        let next = navigation.begin("https://example.com/next").unwrap();
        assert!(!broker.is_current(epoch));
        navigation.fail_synchronous(next);
        assert!(broker.is_current(epoch));
        assert!(!navigation.matches_activity(activity));
        token.store(false, Ordering::Release);
        assert!(!broker.is_current(epoch));
        token.store(true, Ordering::Release);
        permit.revoke();
        assert!(!broker.is_current(epoch));
    }

    #[test]
    fn selection_count_obeys_input_and_resource_limits() {
        assert!(!valid_selection_count(0, true));
        assert!(valid_selection_count(1, false));
        assert!(!valid_selection_count(2, false));
        assert!(valid_selection_count(MAX_SELECTED_ENTRIES, true));
        assert!(!valid_selection_count(MAX_SELECTED_ENTRIES + 1, true));
    }
}
