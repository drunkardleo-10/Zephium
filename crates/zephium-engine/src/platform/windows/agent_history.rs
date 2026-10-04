//! Exact native history identities for retained Windows Work pages.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]
use std::{cell::RefCell, rc::Rc};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl,
};
use windows_core::{HRESULT, HSTRING, PCWSTR};
use zephium_agentic::ContextNavigationTarget;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AgentHistoryBackTicket {
    attempt: u16,
    source: i64,
    destination: i64,
}
#[derive(Clone)]
struct Entry {
    id: i64,
    target: Option<ContextNavigationTarget>,
}
#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    cursor: Option<usize>,
    attempts: u16,
    pending: Option<AgentHistoryBackTicket>,
    calls: usize,
    revision: u64,
    settled: bool,
    failed: bool,
}
#[derive(Clone, Default)]
pub(super) struct AgentHistoryLedger(Rc<RefCell<State>>);
impl AgentHistoryLedger {
    pub(super) fn healthy(&self) -> bool {
        self.0.try_borrow().is_ok_and(|s| !s.failed && s.calls == 0)
    }
    pub(super) fn drained(&self) -> bool {
        self.0.try_borrow().is_ok_and(|s| s.calls == 0)
    }
    pub(super) fn clear(&self) -> Result<(), ()> {
        let mut s = self.0.try_borrow_mut().map_err(|_| ())?;
        if s.calls != 0 {
            return Err(());
        }
        *s = State::default();
        Ok(())
    }
    pub(super) fn enroll(
        &self,
        core: &ICoreWebView2,
        target: ContextNavigationTarget,
        notify: Rc<dyn Fn()>,
        failure: Rc<dyn Fn()>,
    ) -> Result<(), ()> {
        let revision = {
            let mut s = self.0.try_borrow_mut().map_err(|_| ())?;
            if s.pending.is_some() || s.entries.len() >= 32 || s.calls != 0 || s.failed {
                return Err(());
            }
            s.revision = s.revision.checked_add(1).ok_or(())?;
            s.calls += 1;
            s.revision
        };
        let ledger = self.clone();
        let fail = failure.clone();
        let notify_error = notify.clone();
        let result = call(
            core,
            "Page.getNavigationHistory",
            "{}",
            move |snapshot| {
                let result = (|| {
                    let (current, entries) = snapshot?;
                    let entry = entries.get(current).ok_or(())?;
                    if entry.target.as_ref() != Some(&target) {
                        return Err(());
                    }
                    let mut s = ledger.0.try_borrow_mut().map_err(|_| ())?;
                    s.calls = s.calls.saturating_sub(1);
                    if s.revision != revision {
                        return Err(());
                    }
                    if s.entries.iter().any(|old| old.id == entry.id) {
                        return Err(());
                    }
                    if let Some(cursor) = s.cursor {
                        s.entries.truncate(cursor + 1);
                    }
                    s.entries.push(entry.clone());
                    s.cursor = s.entries.len().checked_sub(1);
                    Ok(())
                })();
                if result.is_err() {
                    if let Ok(mut s) = ledger.0.try_borrow_mut() {
                        s.calls = 0;
                        s.failed = true;
                    }
                    (fail)();
                }
                (notify)();
            },
            failure,
        );
        if result.is_err() {
            let mut s = self.0.borrow_mut();
            s.calls = s.calls.saturating_sub(1);
            s.failed = true;
            drop(s);
            notify_error();
        }
        result
    }
    pub(super) fn authorize(&self) -> Result<AgentHistoryBackTicket, ()> {
        let mut s = self.0.try_borrow_mut().map_err(|_| ())?;
        if s.calls != 0 || s.failed || s.pending.is_some() || s.attempts >= 128 {
            return Err(());
        }
        let cursor = s.cursor.ok_or(())?;
        let source = s.entries.get(cursor).ok_or(())?.id;
        let destination = s
            .entries
            .get(cursor.checked_sub(1).ok_or(())?)
            .ok_or(())?
            .id;
        s.attempts += 1;
        let ticket = AgentHistoryBackTicket {
            attempt: s.attempts,
            source,
            destination,
        };
        s.pending = Some(ticket);
        s.settled = false;
        Ok(ticket)
    }
    pub(super) fn target(&self, ticket: AgentHistoryBackTicket) -> Option<ContextNavigationTarget> {
        let s = self.0.try_borrow().ok()?;
        if s.pending != Some(ticket) {
            return None;
        }
        s.entries
            .iter()
            .find(|e| e.id == ticket.destination)
            .and_then(|e| e.target.clone())
    }
    pub(super) fn dispatch(
        &self,
        core: &ICoreWebView2,
        ticket: AgentHistoryBackTicket,
        authority: Rc<dyn Fn() -> bool>,
        notify: Rc<dyn Fn()>,
        failure: Rc<dyn Fn()>,
    ) -> bool {
        {
            let Ok(mut s) = self.0.try_borrow_mut() else {
                return false;
            };
            if s.pending != Some(ticket) || s.calls != 0 || s.failed {
                return false;
            }
            s.calls += 1;
        }
        let ledger = self.clone();
        let core_next = core.clone();
        let fail = failure.clone();
        let notify_next = notify.clone();
        let result = call(
            core,
            "Page.getNavigationHistory",
            "{}",
            move |snapshot| {
                let valid = snapshot.is_ok_and(|(current, entries)| {
                    entries.get(current).is_some_and(|e| e.id == ticket.source)
                        && current
                            .checked_sub(1)
                            .and_then(|i| entries.get(i))
                            .is_some_and(|e| e.id == ticket.destination)
                });
                let pending = ledger
                    .0
                    .try_borrow()
                    .is_ok_and(|s| s.pending == Some(ticket));
                if !valid || !pending || !authority() {
                    if let Ok(mut s) = ledger.0.try_borrow_mut() {
                        s.calls = 0;
                        s.failed = true;
                    }
                    fail();
                    notify_next();
                    return;
                }
                let completed = ledger.clone();
                let failed = fail.clone();
                let notification = notify_next.clone();
                let params = format!("{{\"entryId\":{}}}", ticket.destination);
                let result = call_raw(
                    &core_next,
                    "Page.navigateToHistoryEntry",
                    &params,
                    move |result| {
                        if let Ok(mut s) = completed.0.try_borrow_mut() {
                            s.calls = s.calls.saturating_sub(1);
                            if result.is_err() {
                                s.failed = true;
                                failed();
                            }
                        }
                        notification();
                    },
                    fail.clone(),
                );
                if result.is_err() {
                    if let Ok(mut s) = ledger.0.try_borrow_mut() {
                        s.calls = 0;
                        s.failed = true;
                    }
                    fail();
                    notify_next();
                }
            },
            failure,
        );
        if result.is_err() {
            if let Ok(mut s) = self.0.try_borrow_mut() {
                s.calls = 0;
                s.failed = true;
            }
            notify();
        }
        result.is_ok()
    }
    pub(super) fn ready_for_settlement(
        &self,
        core: &ICoreWebView2,
        notify: Rc<dyn Fn()>,
        failure: Rc<dyn Fn()>,
    ) -> bool {
        let ticket = {
            let Ok(mut s) = self.0.try_borrow_mut() else {
                return false;
            };
            if s.failed || s.calls != 0 {
                return false;
            }
            if s.settled {
                return true;
            }
            let Some(ticket) = s.pending else {
                return false;
            };
            s.calls += 1;
            ticket
        };
        let ledger = self.clone();
        let failed = failure.clone();
        let rejected = notify.clone();
        if call(
            core,
            "Page.getNavigationHistory",
            "{}",
            move |snapshot| {
                let valid = snapshot.is_ok_and(|(current, entries)| {
                    entries
                        .get(current)
                        .is_some_and(|e| e.id == ticket.destination)
                });
                if let Ok(mut s) = ledger.0.try_borrow_mut() {
                    s.calls = s.calls.saturating_sub(1);
                    s.settled = valid && s.pending == Some(ticket);
                    s.failed = !valid;
                }
                if !valid {
                    failed();
                }
                notify();
            },
            failure,
        )
        .is_err()
        {
            if let Ok(mut s) = self.0.try_borrow_mut() {
                s.calls = 0;
                s.failed = true;
            }
            rejected();
        }
        false
    }
    pub(super) fn settle(&self, ticket: AgentHistoryBackTicket) -> Result<(), ()> {
        let mut s = self.0.try_borrow_mut().map_err(|_| ())?;
        if s.pending != Some(ticket) || !s.settled || s.calls != 0 || s.failed {
            return Err(());
        }
        let cursor = s.cursor.ok_or(())?.checked_sub(1).ok_or(())?;
        if s.entries
            .get(cursor)
            .is_none_or(|e| e.id != ticket.destination)
        {
            return Err(());
        }
        s.cursor = Some(cursor);
        s.pending = None;
        s.settled = false;
        Ok(())
    }
    pub(super) fn refuse(&self, ticket: AgentHistoryBackTicket) -> bool {
        self.0.try_borrow_mut().is_ok_and(|mut s| {
            if s.pending != Some(ticket) {
                return false;
            }
            s.pending = None;
            s.failed = true;
            true
        })
    }
}
fn parse_snapshot(raw: String) -> Result<(usize, Vec<Entry>), ()> {
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|_| ())?;
    let current = value
        .get("currentIndex")
        .and_then(|v| v.as_u64())
        .and_then(|i| usize::try_from(i).ok())
        .ok_or(())?;
    let values = value
        .get("entries")
        .and_then(|v| v.as_array())
        .filter(|v| v.len() <= 512)
        .ok_or(())?;
    let mut entries = Vec::with_capacity(values.len());
    for value in values {
        let id = value.get("id").and_then(|v| v.as_i64()).ok_or(())?;
        if entries.iter().any(|e: &Entry| e.id == id) {
            return Err(());
        }
        let url = value.get("url").and_then(|v| v.as_str()).ok_or(())?;
        // The bootstrap entry has no host-authorized document target and can
        // never become an enrolled destination, even if the shared URL type
        // permits about:blank for native construction.
        let target = (url != "about:blank")
            .then(|| ContextNavigationTarget::parse(url).ok())
            .flatten();
        entries.push(Entry { id, target });
    }
    if current >= entries.len() {
        return Err(());
    }
    Ok((current, entries))
}
fn call(
    core: &ICoreWebView2,
    method: &str,
    parameters: &str,
    completion: impl FnOnce(Result<(usize, Vec<Entry>), ()>) + 'static,
    failure: Rc<dyn Fn()>,
) -> Result<(), ()> {
    call_raw(
        core,
        method,
        parameters,
        move |result| completion(result.and_then(parse_snapshot)),
        failure,
    )
}
type Completion = Box<dyn FnOnce(Result<String, ()>)>;
#[windows_core::implement(ICoreWebView2CallDevToolsProtocolMethodCompletedHandler)]
struct BoundedCompletion {
    completion: RefCell<Option<Completion>>,
    failure: Rc<dyn Fn()>,
}
impl ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl for BoundedCompletion_Impl {
    fn Invoke(&self, result: HRESULT, response: &PCWSTR) -> windows_core::Result<()> {
        let outcome = if result.is_err() {
            Err(())
        } else {
            bounded_response(response).ok_or(())
        };
        let completion = self
            .completion
            .try_borrow_mut()
            .ok()
            .and_then(|mut c| c.take());
        if let Some(completion) = completion {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(outcome)))
                .is_err()
            {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.failure)()));
            }
        } else {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.failure)()));
        }
        Ok(())
    }
}
fn bounded_response(raw: &PCWSTR) -> Option<String> {
    let p = raw.as_ptr();
    if p.is_null() {
        return None;
    }
    for len in 0..=262144 {
        // SAFETY: WebView2 provides a NUL-terminated string valid through this callback; scan has a fixed ceiling.
        if unsafe { p.add(len).read() } == 0 {
            // SAFETY: bounded scan proved the initialized prefix.
            let s = String::from_utf16(unsafe { std::slice::from_raw_parts(p, len) }).ok()?;
            return (s.len() <= 262144).then_some(s);
        }
    }
    None
}
fn call_raw(
    core: &ICoreWebView2,
    method: &str,
    parameters: &str,
    completion: impl FnOnce(Result<String, ()>) + 'static,
    failure: Rc<dyn Fn()>,
) -> Result<(), ()> {
    let handler: ICoreWebView2CallDevToolsProtocolMethodCompletedHandler = BoundedCompletion {
        completion: RefCell::new(Some(Box::new(completion))),
        failure,
    }
    .into();
    let method = HSTRING::from(method);
    let parameters = HSTRING::from(parameters); // SAFETY: exact STA-owned core and immutable argument buffers; COM retains the bounded completion owner until callback return.
    unsafe { core.CallDevToolsProtocolMethod(&method, &parameters, &handler) }.map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ledger() -> AgentHistoryLedger {
        let ledger = AgentHistoryLedger::default();
        let mut state = ledger.0.borrow_mut();
        state.entries = vec![
            Entry {
                id: 11,
                target: Some(ContextNavigationTarget::parse("https://example.test/a").unwrap()),
            },
            Entry {
                id: 29,
                target: Some(ContextNavigationTarget::parse("https://example.test/b").unwrap()),
            },
        ];
        state.cursor = Some(1);
        drop(state);
        ledger
    }
    #[test]
    fn back_ticket_keeps_exact_native_identity_and_moves_cursor_only_after_settlement() {
        let ledger = ledger();
        let ticket = ledger.authorize().unwrap();
        assert_eq!((ticket.source, ticket.destination), (29, 11));
        assert!(ledger.authorize().is_err());
        assert!(ledger.settle(ticket).is_err());
        assert_eq!(ledger.0.borrow().cursor, Some(1));
        ledger.0.borrow_mut().settled = true;
        assert!(ledger.settle(ticket).is_ok());
        assert_eq!(ledger.0.borrow().cursor, Some(0));
        assert!(ledger.authorize().is_err());
    }
    #[test]
    fn physical_query_and_failed_traversal_close_new_authority() {
        let ledger = ledger();
        ledger.0.borrow_mut().calls = 1;
        assert!(!ledger.drained());
        assert!(ledger.clear().is_err());
        assert!(ledger.authorize().is_err());
        ledger.0.borrow_mut().calls = 0;
        let ticket = ledger.authorize().unwrap();
        assert!(ledger.refuse(ticket));
        assert!(!ledger.healthy());
        assert_eq!(ledger.0.borrow().cursor, Some(1));
        assert!(ledger.authorize().is_err());
    }
    #[test]
    fn native_history_parser_preserves_bootstrap_and_rejects_duplicate_ids_or_wrong_index() {
        let parsed=parse_snapshot(r#"{"currentIndex":1,"entries":[{"id":2,"url":"about:blank"},{"id":3,"url":"https://example.test/a"}]}"#.into()).unwrap();
        assert_eq!(parsed.1[0].id, 2);
        assert!(parsed.1[0].target.is_none());
        assert!(parse_snapshot(r#"{"currentIndex":1,"entries":[{"id":2,"url":"https://example.test/a"},{"id":2,"url":"https://example.test/b"}]}"#.into()).is_err());
        assert!(parse_snapshot(r#"{"currentIndex":9,"entries":[]}"#.into()).is_err());
    }
}
