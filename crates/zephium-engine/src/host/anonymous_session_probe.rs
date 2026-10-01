//! Release-excluded native cookie continuity and isolation check.
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_foundation::{
    NSArray, NSDate, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieName, NSHTTPCookiePath,
    NSHTTPCookiePropertyKey, NSHTTPCookieValue, NSMutableDictionary, NSRunLoop, NSString,
};
use std::{
    cell::RefCell,
    ptr::NonNull,
    rc::Rc,
    time::{Duration, Instant},
};
use zephium_agentic::{WorkBrowserSession, WorkId};
use zephium_core::ids::ProfileId;

type Store = crate::platform::imp::WebsiteDataStore;

pub(crate) fn run() -> Result<(), &'static str> {
    let profile = ProfileId::generate();
    crate::run_macos_agentic_work_application_probe(profile, move |_| {
        qualify(profile)?;
        Ok(Box::new(|failed| {
            Some(if failed { Err("session_host") } else { Ok(()) })
        }))
    })
}

fn store(session: &WorkBrowserSession) -> Result<Store, &'static str> {
    let result = Rc::new(RefCell::new(None));
    let delivery = result.clone();
    let session = session.clone();
    if !super::try_with(move |host| {
        *delivery.borrow_mut() = Some(host.anonymous_work_store(&session));
    }) {
        return Err("session_dispatch");
    }
    let result = result
        .borrow_mut()
        .take()
        .ok_or("session_not_synchronous")?;
    result.map_err(|_| "session_store")
}

fn wait<T>(result: &Rc<RefCell<Option<T>>>) -> Result<T, &'static str> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let run_loop = NSRunLoop::currentRunLoop();
    loop {
        if let Some(value) = result.borrow_mut().take() {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err("session_cookie_timeout");
        }
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
}

fn contains_cookie(store: &Store) -> Result<bool, &'static str> {
    let result = Rc::new(RefCell::new(None));
    let delivery = result.clone();
    let callback = block2::RcBlock::new(move |cookies: NonNull<NSArray<NSHTTPCookie>>| {
        // SAFETY: WebKit owns this nonnull array through the synchronous callback.
        let cookies = unsafe { cookies.as_ref() };
        let found = cookies.iter().any(|cookie| {
            cookie.name().to_string() == "zeum_session_probe"
                && cookie.value().to_string() == "retained"
        });
        *delivery.borrow_mut() = Some(found);
    });
    // SAFETY: the store and callback remain retained on the owning main thread.
    unsafe {
        store.httpCookieStore().getAllCookies(&callback);
    }
    wait(&result)
}

fn qualify(profile: ProfileId) -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let work = WorkId::generate();
    let session = WorkBrowserSession::new(profile, work, deadline);
    let other = WorkBrowserSession::new(profile, work, deadline);
    let first = store(&session)?;
    let isolated = store(&other)?;
    if Retained::as_ptr(&first) == Retained::as_ptr(&isolated) {
        return Err("session_cross_attempt");
    }
    let name = NSString::from_str("zeum_session_probe");
    let value = NSString::from_str("retained");
    let path = NSString::from_str("/");
    let domain = NSString::from_str("session.zeum.test");
    // SAFETY: Foundation's immutable keys receive the documented string values.
    let cookie = unsafe {
        let properties: Retained<NSMutableDictionary<NSHTTPCookiePropertyKey, AnyObject>> =
            NSMutableDictionary::from_slices(
                &[
                    NSHTTPCookieName,
                    NSHTTPCookieValue,
                    NSHTTPCookiePath,
                    NSHTTPCookieDomain,
                ],
                &[&name, &value, &path, &domain],
            );
        NSHTTPCookie::cookieWithProperties(&properties)
    }
    .ok_or("session_cookie")?;
    let written = Rc::new(RefCell::new(None));
    let delivery = written.clone();
    let callback = block2::RcBlock::new(move || {
        *delivery.borrow_mut() = Some(());
    });
    // SAFETY: exact native anonymous store; all work remains on the main thread.
    unsafe {
        first
            .httpCookieStore()
            .setCookie_completionHandler(&cookie, Some(&callback));
    }
    wait(&written)?;
    drop(first);
    let next = store(&session)?;
    if !contains_cookie(&next)? || contains_cookie(&isolated)? {
        return Err("session_cookie_isolation");
    }
    session.close();
    if store(&session).is_ok() {
        return Err("session_reopened");
    }
    other.close();
    drop(next);
    drop(isolated);
    use std::sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    };
    let cleaned = Arc::new(AtomicU8::new(0));
    let delivery = cleaned.clone();
    dispatch2::DispatchQueue::main().exec_async(move || {
        super::best_effort_with(move |host| {
            delivery.store(
                if host.anonymous_work_stores.is_empty() {
                    1
                } else {
                    2
                },
                Ordering::Release,
            );
        });
    });
    let run_loop = NSRunLoop::currentRunLoop();
    let cleanup_deadline = Instant::now() + Duration::from_secs(5);
    while cleaned.load(Ordering::Acquire) == 0 && Instant::now() < cleanup_deadline {
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    if cleaned.load(Ordering::Acquire) != 1 {
        return Err("session_cleanup");
    }
    Ok(())
}
