use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use zephium_core::ids::ItemId;
use zephium_core::navigation;
use zephium_core::ports::engine::EngineEvent;

use crate::navigation_epoch::{NavigationEpoch, NavigationEpochTracker};

#[cfg(target_os = "macos")]
use super::dispatch::with_extension_background_wake;
use super::dispatch::{
    with_extension_permit_invalidation, with_navigation_commit, with_navigation_settlement,
};

#[derive(Clone)]
pub(super) struct Sink(crate::EngineEventIngressSink);

impl Sink {
    pub(super) fn new(inner: crate::EngineEventIngressSink) -> Self {
        Self(inner)
    }

    pub(super) fn emit(&self, ev: EngineEvent) {
        (self.0)(crate::EngineEventIngress::global(ev));
    }

    pub(super) fn emit_for(&self, token: Arc<AtomicBool>, ev: EngineEvent) {
        (self.0)(crate::EngineEventIngress::for_item(ev, token));
    }
}

/// Every native WebView owns one permit identity. A normal view is bound at
/// construction; a warm spare may be bound exactly once when it is adopted.
/// Revocation is terminal, so a callback retained by a dropped same-id view
/// can never acquire the token of a replacement view.
#[derive(Clone)]
pub(super) struct EventPermit {
    state: Arc<Mutex<EventPermitState>>,
}

enum EventPermitState {
    #[cfg(any(not(all(unix, not(target_os = "macos"))), test))]
    Inactive,
    Bound(Weak<AtomicBool>),
    Revoked,
}

impl EventPermit {
    #[cfg(any(not(all(unix, not(target_os = "macos"))), test))]
    pub(super) fn inactive() -> Self {
        Self {
            state: Arc::new(Mutex::new(EventPermitState::Inactive)),
        }
    }

    pub(super) fn bound(token: &Arc<AtomicBool>) -> Self {
        Self {
            state: Arc::new(Mutex::new(EventPermitState::Bound(Arc::downgrade(token)))),
        }
    }

    #[cfg(any(not(all(unix, not(target_os = "macos"))), test))]
    pub(super) fn bind_once(&self, token: &Arc<AtomicBool>) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *state {
            #[cfg(any(not(all(unix, not(target_os = "macos"))), test))]
            EventPermitState::Inactive => {
                *state = EventPermitState::Bound(Arc::downgrade(token));
                true
            }
            EventPermitState::Bound(_) | EventPermitState::Revoked => false,
        }
    }

    pub(super) fn revoke(&self) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = EventPermitState::Revoked;
    }

    pub(super) fn active_token(&self) -> Option<Arc<AtomicBool>> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*state {
            EventPermitState::Bound(token) => token
                .upgrade()
                .filter(|token| token.load(Ordering::Acquire)),
            #[cfg(any(not(all(unix, not(target_os = "macos"))), test))]
            EventPermitState::Inactive => None,
            EventPermitState::Revoked => None,
        }
    }

    pub(super) fn same_generation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub(super) fn matches_token(&self, token: &Arc<AtomicBool>) -> bool {
        self.active_token()
            .is_some_and(|bound| Arc::ptr_eq(&bound, token))
    }

    pub(super) fn allows_navigation(&self, target: &str) -> bool {
        if !navigation::is_allowed_str(target) {
            return false;
        }
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*state {
            #[cfg(any(not(all(unix, not(target_os = "macos"))), test))]
            EventPermitState::Inactive => target == "about:blank",
            EventPermitState::Bound(token) => token
                .upgrade()
                .is_some_and(|token| token.load(Ordering::Acquire)),
            EventPermitState::Revoked => false,
        }
    }

    pub(super) fn emit(&self, sink: &Sink, event: EngineEvent) {
        if let Some(token) = self.active_token() {
            sink.emit_for(token, event);
        }
    }
}

pub(super) fn navigation_callback_matches(
    view_permit: &EventPermit,
    view_navigation: &NavigationEpochTracker,
    source_permit: &EventPermit,
    source_navigation: &NavigationEpochTracker,
    epoch: NavigationEpoch,
) -> bool {
    view_permit.same_generation(source_permit)
        && view_navigation.same_generation(source_navigation)
        && view_navigation.is_current(epoch)
}

pub(super) fn queue_navigation_commit(
    id: ItemId,
    permit: &EventPermit,
    navigation: &NavigationEpochTracker,
    epoch: NavigationEpoch,
) {
    if permit.active_token().is_none() {
        return;
    }
    let queued_permit = permit.clone();
    let queued_navigation = navigation.clone();
    let admitted = with_navigation_commit(id, move |host| {
        if host.rearm_navigation_presentation(id, &queued_permit, &queued_navigation, epoch) {
            host.emit_navigation_observation(id, &queued_permit, &queued_navigation, epoch);
        }
    });
    if !admitted {
        // Wry has already hidden the committed native surface before invoking
        // this callback. Reserved globally-coalesced capacity proves ordinary
        // overload cannot reach this branch; shutdown/sealed-host rejection
        // terminally revokes the orphaned callback generation.
        permit.revoke();
        navigation.revoke();
        eprintln!("security: committed-document presentation gate was not admitted");
    }
}

/// Wakes only matching authenticated document-background runtimes for one
/// exact provisional navigation. This reconstructs the event wake which WebKit
/// provides to service workers but does not reliably provide to its document
/// background compatibility environment.
#[cfg(target_os = "macos")]
pub(super) fn queue_extension_background_wake(
    id: ItemId,
    permit: &EventPermit,
    navigation: &NavigationEpochTracker,
    epoch: NavigationEpoch,
    target: String,
) {
    if permit.active_token().is_none() || !navigation.matches_current_target(epoch, &target) {
        return;
    }
    let queued_permit = permit.clone();
    let queued_navigation = navigation.clone();
    let admitted = with_extension_background_wake(id, move |host| {
        host.wake_matching_document_backgrounds(
            id,
            &queued_permit,
            &queued_navigation,
            epoch,
            &target,
        );
    });
    if !admitted {
        // This is a usability compatibility hint, not a security mutation.
        // WebKit retains its native behavior and the extension call may fail
        // closed; ordinary browsing remains valid.
        eprintln!("engine: matching extension background wake was not admitted");
    }
}

/// A provisional main-frame transition permanently consumes every one-shot
/// document permit issued before it. Its dedicated queue key never crosses an
/// intervening operation. The navigation tracker's synchronous, non-rearmable
/// operation generation remains the primary authority barrier even if host
/// settlement is delayed or refused.
pub(super) fn queue_navigation_authority_invalidation(
    id: ItemId,
    permit: &EventPermit,
    navigation: &NavigationEpochTracker,
) {
    if permit.active_token().is_none() {
        return;
    }
    let queued_permit = permit.clone();
    let queued_navigation = navigation.clone();
    let admitted = with_extension_permit_invalidation(id, move |host| {
        host.invalidate_extension_document_permits_for_navigation(
            id,
            &queued_permit,
            &queued_navigation,
        );
    });
    if !admitted {
        // Losing this mutation could make an old permit valid again after a
        // provisional failure restores its document. Retire the entire native
        // generation instead of accepting that replay window.
        permit.revoke();
        navigation.revoke();
        eprintln!("security: extension document-permit invalidation was not admitted");
    }
}

pub(super) fn queue_navigation_completion(
    id: ItemId,
    permit: &EventPermit,
    navigation: &NavigationEpochTracker,
    epoch: NavigationEpoch,
) {
    if permit.active_token().is_none() {
        return;
    }
    let queued_permit = permit.clone();
    let queued_navigation = navigation.clone();
    with_navigation_settlement(id, move |host| {
        if host.rearm_navigation_presentation(id, &queued_permit, &queued_navigation, epoch)
            && host.emit_navigation_observation(id, &queued_permit, &queued_navigation, epoch)
        {
            host.complete_title_attribution(id, &queued_permit, &queued_navigation, epoch);
            host.emit_navigation_ready(id, &queued_permit, &queued_navigation, epoch);
        }
    });
}

pub(super) fn queue_navigation_failure(
    id: ItemId,
    permit: &EventPermit,
    navigation: &NavigationEpochTracker,
    failed: NavigationEpoch,
    restored: Option<NavigationEpoch>,
) {
    if permit.active_token().is_none() {
        return;
    }
    let queued_permit = permit.clone();
    let queued_navigation = navigation.clone();
    with_navigation_settlement(id, move |host| {
        host.settle_navigation_failure(id, &queued_permit, &queued_navigation, failed, restored);
    });
}

#[cfg(test)]
mod tests;
