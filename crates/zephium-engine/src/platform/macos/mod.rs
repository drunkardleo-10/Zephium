mod native;
mod navigation;
mod stage;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

pub use native::{add_user_script, configure, query_document_activity, stop_loading};
pub use navigation::NavigationObserver;
use objc2::rc::Retained;
use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};
pub use stage::ContentStage;
pub type InstalledNavigationObserver = objc2::rc::Retained<NavigationObserver>;

const PAGE_URL_UTF16_LIMIT: usize = 8 * 1_024;
const PAGE_URL_UTF8_LIMIT: usize = 8 * 1_024;

pub fn install_navigation_observer(
    view: &wry::WebView,
    on_change: impl Fn() + 'static,
) -> Result<InstalledNavigationObserver, &'static str> {
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().ok_or("WKWebView observer requires the main thread")?;
    Ok(NavigationObserver::install(
        mtm,
        &native::webkit(view),
        on_change,
    ))
}

pub fn current_url(view: &wry::WebView) -> Option<String> {
    let url = unsafe { native::webkit(view).URL() }?;
    let value = url.absoluteString()?;
    if value.length() > PAGE_URL_UTF16_LIMIT {
        return None;
    }
    let value = value.to_string();
    (value.len() <= PAGE_URL_UTF8_LIMIT).then_some(value)
}

pub fn enforce_navigation_pending(view: &wry::WebView) -> bool {
    view.set_visible(false).is_ok()
}

pub(crate) type WebsiteDataStore = Retained<WKWebsiteDataStore>;

/// Allocate one non-persistent store for a private profile. The host retains
/// this object independently of every view so all tabs in that profile share
/// cookies and origin storage, while a construction failure cannot make the
/// native erasure obligation disappear.
pub(crate) fn new_ephemeral_data_store() -> Result<WebsiteDataStore, &'static str> {
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().ok_or("WKWebsiteDataStore requires the main thread")?;
    let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
    validate_ephemeral_data_store(&store)?;
    Ok(store)
}

/// Build a fresh mutable configuration for one WKWebView while binding it to
/// the profile-owned store. Configurations themselves must never be shared:
/// Wry installs per-view scripts, delegates and scheme handlers into them.
pub(crate) fn new_configuration_with_data_store(
    store: &WebsiteDataStore,
) -> Result<Retained<WKWebViewConfiguration>, &'static str> {
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().ok_or("WKWebViewConfiguration requires the main thread")?;
    validate_ephemeral_data_store(store)?;
    // SAFETY: `mtm` proves AppKit/WebKit main-thread affinity. `new` returns
    // an owned Objective-C object and has no additional preconditions.
    let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe { configuration.setWebsiteDataStore(store) };
    let configured_store = unsafe { configuration.websiteDataStore() };
    if Retained::as_ptr(&configured_store) != Retained::as_ptr(store) {
        return Err("WKWebViewConfiguration did not retain the requested website data store");
    }
    Ok(configuration)
}

fn validate_ephemeral_data_store(store: &WebsiteDataStore) -> Result<(), &'static str> {
    if unsafe { store.isPersistent() } {
        return Err("private profile received a persistent WKWebsiteDataStore");
    }
    if unsafe { store.identifier() }.is_some() {
        return Err("private profile received an identifiable WKWebsiteDataStore");
    }
    Ok(())
}

#[derive(Clone)]
struct ProfileErasure {
    profile: zephium_core::ids::ProfileId,
    completion: Arc<crate::erasure::Completion>,
    attempt: Arc<AtomicBool>,
}

impl ProfileErasure {
    fn new(
        profile: zephium_core::ids::ProfileId,
        completion: Arc<crate::erasure::Completion>,
    ) -> Self {
        Self {
            profile,
            attempt: completion.attempt_flag(),
            completion,
        }
    }

    fn finish(&self, outcome: zephium_core::ports::engine::ProfileDataErasureOutcome) {
        self.completion.finish(outcome);
        if outcome == zephium_core::ports::engine::ProfileDataErasureOutcome::Verified {
            // `finish` first marks this exact attempt inactive. The host then
            // generation-checks the Arc before releasing its last strong
            // store handle, so a late callback cannot erase a retry's proof.
            crate::host::release_macos_erasure_obligation(self.profile, self.attempt.clone());
        }
    }

    fn report_unsettled(&self, outcome: zephium_core::ports::engine::ProfileDataErasureOutcome) {
        self.completion.report_unsettled(outcome);
    }
}

/// Remove a profile's named WKWebsiteDataStore only after the host has
/// released every WKWebView using it. Apple reports completion errors, but a
/// successful callback alone is not our proof boundary: enumerate the app's
/// identifiers again and acknowledge only when this identifier is absent.
pub(crate) fn erase_profile_data(
    profile: zephium_core::ids::ProfileId,
    mut ephemeral_stores: Vec<WebsiteDataStore>,
    completion: Arc<crate::erasure::Completion>,
) {
    let erasure = ProfileErasure::new(profile, completion);
    let mut seen = std::collections::HashSet::new();
    ephemeral_stores.retain(|store| seen.insert(Retained::as_ptr(store) as usize));
    if ephemeral_stores.is_empty() {
        erase_named_profile_data(erasure);
        return;
    }

    let remaining = Arc::new(AtomicUsize::new(ephemeral_stores.len()));
    let failed = Arc::new(AtomicBool::new(false));
    for store in ephemeral_stores {
        clear_and_verify_ephemeral_store(store, remaining.clone(), failed.clone(), erasure.clone());
    }
}

fn clear_and_verify_ephemeral_store(
    store: WebsiteDataStore,
    remaining: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
    erasure: ProfileErasure,
) {
    use objc2_foundation::{MainThreadMarker, NSDate};

    let Some(mtm) = MainThreadMarker::new() else {
        failed.store(true, Ordering::Release);
        complete_ephemeral_store(remaining, failed, erasure);
        return;
    };
    let data_types = unsafe { WKWebsiteDataStore::allWebsiteDataTypes(mtm) };
    let epoch = NSDate::dateWithTimeIntervalSince1970(0.0);
    let verify_store = store.clone();
    let verify_types = data_types.clone();
    let removed = block2::RcBlock::new(move || {
        use objc2_foundation::NSArray;
        use objc2_web_kit::WKWebsiteDataRecord;

        let retained_store = verify_store.clone();
        let remaining = remaining.clone();
        let failed = failed.clone();
        let erasure = erasure.clone();
        let fetched = block2::RcBlock::new(
            move |records: std::ptr::NonNull<NSArray<WKWebsiteDataRecord>>| {
                let empty = unsafe { records.as_ref().count() == 0 };
                // A borrow is enough to keep the captured strong reference
                // alive while preserving the Fn (not FnOnce) block ABI.
                let _ = &retained_store;
                if !empty {
                    failed.store(true, Ordering::Release);
                }
                complete_ephemeral_store(remaining.clone(), failed.clone(), erasure.clone());
            },
        );
        unsafe {
            verify_store.fetchDataRecordsOfTypes_completionHandler(&verify_types, &fetched);
        }
    });
    unsafe {
        store.removeDataOfTypes_modifiedSince_completionHandler(&data_types, &epoch, &removed);
    }
}

fn complete_ephemeral_store(
    remaining: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
    erasure: ProfileErasure,
) {
    if remaining.fetch_sub(1, Ordering::AcqRel) != 1 {
        return;
    }
    if failed.load(Ordering::Acquire) {
        // An ephemeral store has no durable identifier that a later attempt
        // can rediscover. Dropping its last handle and releasing admission
        // would let an empty retry forget this failed native obligation and
        // falsely report Verified. Report the failure once but keep the
        // process-lifetime attempt debt occupied; restart is the only safe
        // recovery until WebKit has provided a positive clear/fetch proof.
        erasure.report_unsettled(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
    } else {
        erase_named_profile_data(erasure);
    }
}

fn erase_named_profile_data(erasure: ProfileErasure) {
    use wry::WebViewExtDarwin;

    let identifier = erasure.profile.bytes();
    let initial_erasure = erasure.clone();
    let result =
        <wry::WebView as WebViewExtDarwin>::fetch_data_store_identifiers(move |identifiers| {
            if !identifiers.contains(&identifier) {
                initial_erasure
                    .finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
                return;
            }

            let removed_erasure = initial_erasure.clone();
            <wry::WebView as WebViewExtDarwin>::remove_data_store(&identifier, move |removed| {
                if removed.is_err() {
                    removed_erasure
                        .finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
                    return;
                }

                let verified_erasure = removed_erasure.clone();
                if <wry::WebView as WebViewExtDarwin>::fetch_data_store_identifiers(
                    move |identifiers| {
                        let outcome = if identifiers.contains(&identifier) {
                            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
                        } else {
                            zephium_core::ports::engine::ProfileDataErasureOutcome::Verified
                        };
                        verified_erasure.finish(outcome);
                    },
                )
                .is_err()
                {
                    removed_erasure
                        .finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
                }
            });
        });
    if result.is_err() {
        erasure.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn failed_ephemeral_verification_cannot_be_forgotten_by_a_retry() {
        let (tx, rx) = mpsc::channel();
        let attempt = Arc::new(AtomicBool::new(true));
        let completion = crate::erasure::Completion::start(
            Box::new(move |outcome| tx.send(outcome).unwrap()),
            attempt.clone(),
        );

        complete_ephemeral_store(
            Arc::new(AtomicUsize::new(1)),
            Arc::new(AtomicBool::new(true)),
            ProfileErasure::new(zephium_core::ids::ProfileId::from(7), completion),
        );

        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)).unwrap(),
            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
        );
        assert!(attempt.load(Ordering::Acquire));
    }
}
