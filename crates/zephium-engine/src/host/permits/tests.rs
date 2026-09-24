use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::navigation_epoch::NavigationEpochTracker;

use super::{navigation_callback_matches, EventPermit};

#[test]
fn native_event_permit_is_one_shot_and_generation_exact() {
    let first = Arc::new(AtomicBool::new(true));
    let replacement = Arc::new(AtomicBool::new(true));
    let permit = EventPermit::inactive();
    let callback_copy = permit.clone();

    assert!(permit.bind_once(&first));
    assert!(permit.matches_token(&first));
    assert!(callback_copy.same_generation(&permit));
    assert!(!callback_copy.bind_once(&replacement));
    assert!(!permit.matches_token(&replacement));

    permit.revoke();
    assert!(callback_copy.active_token().is_none());
    assert!(!permit.bind_once(&replacement));

    let replacement_permit = EventPermit::bound(&replacement);
    assert!(!replacement_permit.same_generation(&permit));
    assert!(replacement_permit.matches_token(&replacement));
}

#[test]
fn revoking_an_inactive_spare_is_terminal() {
    let token = Arc::new(AtomicBool::new(true));
    let permit = EventPermit::inactive();
    let callback_copy = permit.clone();

    assert!(permit.allows_navigation("about:blank"));
    permit.revoke();

    assert!(!callback_copy.allows_navigation("about:blank"));
    assert!(!callback_copy.bind_once(&token));
    assert!(permit.active_token().is_none());
}

#[test]
fn navigation_callbacks_require_generation_tracker_and_epoch_identity() {
    let token = Arc::new(AtomicBool::new(true));
    let permit = EventPermit::bound(&token);
    let callback_permit = permit.clone();
    let navigation = NavigationEpochTracker::new();
    let callback_navigation = navigation.clone();
    let first = navigation
        .begin("https://example.com/first")
        .expect("first navigation epoch");

    assert!(navigation_callback_matches(
        &permit,
        &navigation,
        &callback_permit,
        &callback_navigation,
        first,
    ));

    let second = navigation
        .begin("https://example.com/second")
        .expect("second navigation epoch");
    assert!(!navigation_callback_matches(
        &permit,
        &navigation,
        &callback_permit,
        &callback_navigation,
        first,
    ));
    assert!(navigation_callback_matches(
        &permit,
        &navigation,
        &callback_permit,
        &callback_navigation,
        second,
    ));

    // The same outer token does not make a replacement native generation
    // or a distinct epoch tracker equivalent to the callback owner.
    let replacement_permit = EventPermit::bound(&token);
    assert!(!navigation_callback_matches(
        &replacement_permit,
        &navigation,
        &callback_permit,
        &callback_navigation,
        second,
    ));
    let replacement_navigation = NavigationEpochTracker::new();
    let coincident = replacement_navigation
        .begin("https://example.com/second")
        .expect("replacement navigation epoch");
    assert!(!navigation_callback_matches(
        &permit,
        &replacement_navigation,
        &callback_permit,
        &callback_navigation,
        coincident,
    ));
}

#[test]
fn a_download_only_view_cannot_regain_page_authority() {
    let first = Arc::new(AtomicBool::new(true));
    let second = Arc::new(AtomicBool::new(true));
    let permit = EventPermit::bound(&first);
    assert!(permit.retire_for_download());
    assert!(permit.active_token().is_none());
    assert!(permit.allows_navigation("about:blank"));
    assert!(!permit.allows_navigation("https://example.com"));
    assert!(!permit.bind_once(&second));
    assert!(!permit.retire_for_download());
    permit.revoke();
    assert!(!permit.allows_navigation("about:blank"));
}
