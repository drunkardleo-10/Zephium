//! Whether this launch opens onboarding instead of the browser.
//!
//! Native asks once, before the window loads either page, so the browser
//! never carries onboarding and onboarding never waits on the browser.

use zephium_core::ports::store::{ProfileDeletionLoad, SessionLoad, Store};

const KEY: &str = "onboarding";

/// True on a clean first run, and on every launch after one until it is
/// finished. The answer is recorded the first time it is worked out, so an
/// install pays for reading its session here once, not on every launch.
pub fn onboarding_due<S: Store + ?Sized>(store: &S) -> bool {
    match store.app_setting(KEY).as_deref() {
        Some("pending") => return true,
        Some(_) => return false,
        None => {}
    }
    // Mirrors what bootstrap accepts as a first run: no session and nothing
    // journaled against one. Recovery and failed reads never begin it, and
    // are left unrecorded so a later launch decides again.
    let journal_empty = matches!(
        store.pending_profile_deletions(),
        ProfileDeletionLoad::Loaded(pending) if pending.is_empty()
    );
    match store.load_session() {
        SessionLoad::Absent if journal_empty => {
            let _ = store.set_app_setting(KEY.into(), "pending".into());
            true
        }
        // An install from before onboarding existed keeps its browser.
        SessionLoad::Loaded { .. } | SessionLoad::LoadedWithDegradedProfiles { .. } => {
            let _ = store.set_app_setting(KEY.into(), "done".into());
            false
        }
        _ => false,
    }
}

/// Records onboarding as finished, so the next launch opens the browser.
pub fn finish_onboarding<S: Store + ?Sized>(store: &S) -> bool {
    store.set_app_setting(KEY.into(), "done".into())
}
