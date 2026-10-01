//! Threaded race kept outside the zero-idle functional core sources.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use zephium_agentic::{WorkBrowserSession, WorkId};

#[test]
fn anonymous_session_close_racing_registration_cannot_lose_retirement() {
    for _ in 0..32 {
        let session = WorkBrowserSession::new(
            1_u128.into(),
            WorkId::from(2_u128),
            Instant::now() + Duration::from_secs(60),
        );
        let child = session.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let thread = std::thread::spawn(move || {
            child.register_retirement(Box::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            }))
        });
        session.close();
        let registered = thread.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(registered));
        assert!(!session.is_current());
    }
}
