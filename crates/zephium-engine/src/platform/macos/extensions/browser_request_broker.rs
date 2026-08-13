//! Bounded, exactly-once native completion ownership for browser mutations.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::{Retained, Weak};
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{WKWebExtensionContext, WKWebExtensionController, WKWebExtensionTab};
use zephium_core::extensions::{
    ExtensionBrowserRequest, ExtensionBrowserRequestAction, ExtensionBrowserRequestId,
    ExtensionBrowserRequestRejection, ExtensionBrowserRequestResult,
    ExtensionBrowserRequestSettlement, MAX_PENDING_EXTENSION_BROWSER_REQUESTS,
    MAX_PENDING_EXTENSION_BROWSER_REQUESTS_PER_PROFILE,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::engine::EngineEvent;

use crate::{EngineEventIngress, EngineEventIngressSink};

const BROWSER_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const BROWSER_REQUEST_ERROR_DOMAIN: &str = "app.zephium.extension-browser";

type TabCompletion = RcBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)>;
type UnitCompletion = RcBlock<dyn Fn(*mut NSError)>;

pub(super) struct BrowserRequestPool {
    pending: Cell<usize>,
}

impl BrowserRequestPool {
    pub(super) const fn new() -> Self {
        Self {
            pending: Cell::new(0),
        }
    }

    fn try_reserve(&self) -> bool {
        let pending = self.pending.get();
        if pending >= MAX_PENDING_EXTENSION_BROWSER_REQUESTS {
            return false;
        }
        self.pending.set(pending + 1);
        true
    }

    fn release(&self) {
        let pending = self.pending.get();
        debug_assert!(pending > 0);
        self.pending.set(pending.saturating_sub(1));
    }

    #[cfg(test)]
    pub(super) fn pending(&self) -> usize {
        self.pending.get()
    }
}

enum PendingCompletion {
    Tab(TabCompletion),
    Unit(UnitCompletion),
}

struct PendingRequest {
    completion: PendingCompletion,
    watchdog: crate::platform::imp::ContentPolicyTimeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BrowserRequestSettlementOutcome {
    Settled,
    Stale,
    IntegrityFailed,
}

/// One profile controller's request broker. It owns no browser state and can
/// only ask the Shell to mutate its authoritative model.
pub(super) struct BrowserRequestBroker {
    profile: ProfileId,
    sink: Option<EngineEventIngressSink>,
    controller: RefCell<Option<Weak<WKWebExtensionController>>>,
    pool: Rc<BrowserRequestPool>,
    next_request: Cell<u64>,
    pending: RefCell<HashMap<ExtensionBrowserRequestId, PendingRequest>>,
    sealed: Cell<bool>,
    discarded_tab_webview_refusals: Cell<u64>,
}

impl BrowserRequestBroker {
    pub(super) fn new(
        profile: ProfileId,
        sink: Option<EngineEventIngressSink>,
        pool: Rc<BrowserRequestPool>,
    ) -> Rc<Self> {
        Rc::new(Self {
            profile,
            sink,
            controller: RefCell::new(None),
            pool,
            next_request: Cell::new(1),
            pending: RefCell::new(HashMap::new()),
            sealed: Cell::new(false),
            discarded_tab_webview_refusals: Cell::new(0),
        })
    }

    pub(super) fn record_discarded_tab_webview_refusal(&self) {
        self.discarded_tab_webview_refusals
            .set(self.discarded_tab_webview_refusals.get().saturating_add(1));
    }

    #[cfg(any(test, feature = "native-web-extension-probes"))]
    pub(super) fn discarded_tab_webview_refusals(&self) -> u64 {
        self.discarded_tab_webview_refusals.get()
    }

    pub(super) fn bind_controller(&self, controller: &Retained<WKWebExtensionController>) {
        *self.controller.borrow_mut() = Some(Weak::from_retained(controller));
    }

    pub(super) fn accepts(
        &self,
        controller: Option<&WKWebExtensionController>,
        context: &WKWebExtensionContext,
    ) -> bool {
        if self.sealed.get() {
            return false;
        }
        let Some(expected) = self.controller.borrow().as_ref().and_then(Weak::load) else {
            return false;
        };
        if controller.is_some_and(|actual| !std::ptr::eq(&*expected, actual)) {
            return false;
        }
        // SAFETY: the broker and controller are main-thread-owned. Membership
        // binds the callback to a context loaded by this exact profile owner.
        unsafe { expected.extensionContexts() }.containsObject(context)
    }

    pub(super) fn notify_actions_invalidated(&self) {
        let Some(sink) = self.sink.as_ref() else {
            return;
        };
        let profile = self.profile;
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            sink(EngineEventIngress::global(
                EngineEvent::ExtensionActionsInvalidated { profile },
            ));
        }));
    }

    pub(super) fn begin_tab(
        &self,
        action: ExtensionBrowserRequestAction,
        completion: &DynBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)>,
    ) {
        self.begin(action, PendingCompletion::Tab(completion.copy()));
    }

    pub(super) fn begin_unit(
        &self,
        action: ExtensionBrowserRequestAction,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        self.begin(action, PendingCompletion::Unit(completion.copy()));
    }

    fn begin(&self, action: ExtensionBrowserRequestAction, completion: PendingCompletion) {
        let Some(sink) = self.sink.as_ref() else {
            complete_rejected(completion, ExtensionBrowserRequestRejection::Unsupported);
            return;
        };
        if self.sealed.get() {
            complete_rejected(completion, ExtensionBrowserRequestRejection::ShuttingDown);
            return;
        }
        if self.pending.borrow().len() >= MAX_PENDING_EXTENSION_BROWSER_REQUESTS_PER_PROFILE
            || !self.pool.try_reserve()
        {
            complete_rejected(
                completion,
                ExtensionBrowserRequestRejection::CapacityExceeded,
            );
            return;
        }

        let Some(id) = self.allocate_request_id() else {
            self.pool.release();
            complete_rejected(
                completion,
                ExtensionBrowserRequestRejection::CapacityExceeded,
            );
            return;
        };
        let request = match ExtensionBrowserRequest::new(self.profile, id, action) {
            Ok(request) => request,
            Err(_) => {
                self.pool.release();
                complete_rejected(completion, ExtensionBrowserRequestRejection::InvalidRequest);
                return;
            }
        };
        let profile = self.profile;
        let Some(watchdog) = crate::platform::imp::schedule_content_policy_timeout(
            BROWSER_REQUEST_TIMEOUT,
            move || {
                let _ = crate::host::with_extension_browser_request_terminal(move |host| {
                    host.timeout_extension_browser_request(profile, id);
                });
            },
        ) else {
            self.pool.release();
            complete_rejected(
                completion,
                ExtensionBrowserRequestRejection::NativeAdmissionFailed,
            );
            return;
        };

        let replaced = self.pending.borrow_mut().insert(
            id,
            PendingRequest {
                completion,
                watchdog,
            },
        );
        debug_assert!(replaced.is_none());
        if replaced.is_some() {
            self.pool.release();
            return;
        }

        let delivered = std::panic::catch_unwind(AssertUnwindSafe(|| {
            sink(EngineEventIngress::global(
                EngineEvent::ExtensionBrowserRequested { request },
            ));
        }))
        .is_ok();
        if !delivered {
            self.reject_exact(id, ExtensionBrowserRequestRejection::NativeAdmissionFailed);
        }
    }

    fn allocate_request_id(&self) -> Option<ExtensionBrowserRequestId> {
        for _ in 0..=MAX_PENDING_EXTENSION_BROWSER_REQUESTS_PER_PROFILE {
            let candidate = self.next_request.get();
            self.next_request
                .set(candidate.checked_add(1).unwrap_or(1).max(1));
            let Some(candidate) = ExtensionBrowserRequestId::new(candidate) else {
                continue;
            };
            if !self.pending.borrow().contains_key(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    pub(super) fn settle(
        &self,
        id: ExtensionBrowserRequestId,
        settlement: ExtensionBrowserRequestSettlement,
        created_tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
    ) -> BrowserRequestSettlementOutcome {
        let Some(pending) = self.take(id) else {
            return BrowserRequestSettlementOutcome::Stale;
        };
        drop(pending.watchdog);
        match (pending.completion, settlement) {
            (
                PendingCompletion::Unit(completion),
                ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete),
            ) => {
                completion.call((std::ptr::null_mut(),));
                BrowserRequestSettlementOutcome::Settled
            }
            (
                PendingCompletion::Tab(completion),
                ExtensionBrowserRequestSettlement::Applied(
                    ExtensionBrowserRequestResult::CreatedTab(_),
                ),
            ) => {
                let Some(tab) = created_tab else {
                    complete_rejected(
                        PendingCompletion::Tab(completion),
                        ExtensionBrowserRequestRejection::NativeAdmissionFailed,
                    );
                    return BrowserRequestSettlementOutcome::IntegrityFailed;
                };
                completion.call((
                    (tab as *const ProtocolObject<_>).cast_mut(),
                    std::ptr::null_mut(),
                ));
                BrowserRequestSettlementOutcome::Settled
            }
            (completion, ExtensionBrowserRequestSettlement::Rejected(reason)) => {
                complete_rejected(completion, reason);
                BrowserRequestSettlementOutcome::Settled
            }
            (completion, ExtensionBrowserRequestSettlement::Applied(_)) => {
                complete_rejected(
                    completion,
                    ExtensionBrowserRequestRejection::NativeAdmissionFailed,
                );
                BrowserRequestSettlementOutcome::IntegrityFailed
            }
        }
    }

    pub(super) fn timeout(&self, id: ExtensionBrowserRequestId) -> bool {
        let Some(pending) = self.take(id) else {
            return false;
        };
        // The dispatch source is executing now; dropping it still balances
        // ownership and cancellation before the external completion reenters.
        drop(pending.watchdog);
        complete_rejected(
            pending.completion,
            ExtensionBrowserRequestRejection::NativeAdmissionFailed,
        );
        true
    }

    pub(super) fn reject_unit(
        &self,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
        reason: ExtensionBrowserRequestRejection,
    ) {
        complete_rejected(PendingCompletion::Unit(completion.copy()), reason);
    }

    pub(super) fn reject_tab(
        &self,
        completion: &DynBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)>,
        reason: ExtensionBrowserRequestRejection,
    ) {
        complete_rejected(PendingCompletion::Tab(completion.copy()), reason);
    }

    fn reject_exact(
        &self,
        id: ExtensionBrowserRequestId,
        reason: ExtensionBrowserRequestRejection,
    ) {
        if let Some(pending) = self.take(id) {
            drop(pending.watchdog);
            complete_rejected(pending.completion, reason);
        }
    }

    fn take(&self, id: ExtensionBrowserRequestId) -> Option<PendingRequest> {
        let pending = self.pending.borrow_mut().remove(&id)?;
        self.pool.release();
        Some(pending)
    }

    pub(super) fn seal_and_reject(&self) {
        self.sealed.set(true);
        self.controller.borrow_mut().take();
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for (_, pending) in pending {
            self.pool.release();
            drop(pending.watchdog);
            complete_rejected(
                pending.completion,
                ExtensionBrowserRequestRejection::ShuttingDown,
            );
        }
    }
}

impl Drop for BrowserRequestBroker {
    fn drop(&mut self) {
        self.seal_and_reject();
    }
}

fn complete_rejected(completion: PendingCompletion, reason: ExtensionBrowserRequestRejection) {
    let error = browser_request_error(reason);
    let error = Retained::as_ptr(&error).cast_mut();
    match completion {
        PendingCompletion::Tab(completion) => {
            completion.call((std::ptr::null_mut(), error));
        }
        PendingCompletion::Unit(completion) => {
            completion.call((error,));
        }
    }
}

fn browser_request_error(reason: ExtensionBrowserRequestRejection) -> Retained<NSError> {
    let code = match reason {
        ExtensionBrowserRequestRejection::InvalidContext => 1,
        ExtensionBrowserRequestRejection::InvalidRequest => 2,
        ExtensionBrowserRequestRejection::InvalidScope => 3,
        ExtensionBrowserRequestRejection::Unsupported => 4,
        ExtensionBrowserRequestRejection::CapacityExceeded => 5,
        ExtensionBrowserRequestRejection::NativeAdmissionFailed => 6,
        ExtensionBrowserRequestRejection::ShuttingDown => 7,
        ExtensionBrowserRequestRejection::TabDiscarded => 8,
    };
    let domain = NSString::from_str(BROWSER_REQUEST_ERROR_DOMAIN);
    // SAFETY: domain and numeric code are bounded constants and no user data
    // crosses into the native error surface.
    unsafe { NSError::errorWithDomain_code_userInfo(&domain, code, None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn global_pool_enforces_the_exact_process_ceiling() {
        let pool = BrowserRequestPool::new();
        for _ in 0..MAX_PENDING_EXTENSION_BROWSER_REQUESTS {
            assert!(pool.try_reserve());
        }
        assert_eq!(pool.pending(), MAX_PENDING_EXTENSION_BROWSER_REQUESTS);
        assert!(!pool.try_reserve());
        for _ in 0..MAX_PENDING_EXTENSION_BROWSER_REQUESTS {
            pool.release();
        }
        assert_eq!(pool.pending(), 0);
    }

    #[test]
    fn discarded_tab_diagnostic_saturates() {
        let broker =
            BrowserRequestBroker::new(ProfileId::from(1), None, Rc::new(BrowserRequestPool::new()));
        broker.discarded_tab_webview_refusals.set(u64::MAX);
        broker.record_discarded_tab_webview_refusal();
        assert_eq!(broker.discarded_tab_webview_refusals(), u64::MAX);
    }

    #[test]
    fn discarded_tab_rejection_has_a_stable_native_error_code() {
        let error = browser_request_error(ExtensionBrowserRequestRejection::TabDiscarded);
        assert_eq!(error.code(), 8);
    }

    #[test]
    fn action_invalidation_emits_only_the_bounded_profile_fact() {
        let profile = ProfileId::from(3);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = observed.clone();
        let sink: EngineEventIngressSink = Arc::new(move |ingress| {
            if let EngineEvent::ExtensionActionsInvalidated { profile } = ingress.event {
                observed_for_sink.lock().unwrap().push(profile);
            }
        });
        let broker =
            BrowserRequestBroker::new(profile, Some(sink), Rc::new(BrowserRequestPool::new()));

        broker.notify_actions_invalidated();

        assert_eq!(*observed.lock().unwrap(), vec![profile]);
    }

    #[test]
    fn absent_product_sink_completes_with_bounded_error() {
        let broker =
            BrowserRequestBroker::new(ProfileId::from(1), None, Rc::new(BrowserRequestPool::new()));
        let observed = Rc::new(Cell::new(None));
        let callback_observed = observed.clone();
        let completion: UnitCompletion = RcBlock::new(move |error: *mut NSError| {
            callback_observed.set((!error.is_null()).then_some(unsafe { (*error).code() }));
        });
        broker.begin_unit(
            ExtensionBrowserRequestAction::ActivateTab {
                tab: zephium_core::ids::ItemId::from(2),
            },
            &completion,
        );
        assert_eq!(observed.get(), Some(4));
        assert_eq!(broker.pool.pending(), 0);
    }
}
