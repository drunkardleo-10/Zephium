//! Principal-bound, one-shot compatibility broker for reviewed package adapters.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2_foundation::{NSError, NSString, NSUTF8StringEncoding};
use objc2_web_kit::{WKWebExtensionContext, WKWebExtensionController};
use zephium_core::extensions::{
    ExtensionCompatibilityBrokerOperation, ExtensionCompatibilityBrokerRejection,
    ExtensionCompatibilityBrokerRequest, ExtensionCompatibilityBrokerRequestId,
    ExtensionCompatibilityBrokerResult, ExtensionCompatibilityBrokerSettlement,
    ExtensionCompatibilityBrokerWitness, MAX_EXTENSION_COMPATIBILITY_BROKER_RESPONSE_BYTES,
    MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS,
    MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS_PER_PROFILE,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::engine::EngineEvent;

use crate::{EngineEventIngress, EngineEventIngressSink};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const ERROR_DOMAIN: &str = "app.zephium.extension-compatibility-broker";

type Reply = RcBlock<dyn Fn(*mut AnyObject, *mut NSError)>;

pub(super) struct CompatibilityBrokerPool {
    pending: Cell<usize>,
}

impl CompatibilityBrokerPool {
    pub(super) const fn new() -> Self {
        Self {
            pending: Cell::new(0),
        }
    }

    fn try_reserve(&self) -> bool {
        let pending = self.pending.get();
        if pending >= MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS {
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
    fn pending(&self) -> usize {
        self.pending.get()
    }
}

struct PendingRequest {
    context: Weak<WKWebExtensionContext>,
    operation: ExtensionCompatibilityBrokerOperation,
    reply: Reply,
    watchdog: crate::platform::imp::ContentPolicyTimeout,
    dispatched: bool,
    runtime: Option<zephium_core::extensions::ExtensionRuntimeInstance>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CompatibilityBrokerSettlementOutcome {
    Settled,
    Stale,
    IntegrityFailed,
}

pub(super) struct CompatibilityBroker {
    profile: ProfileId,
    sink: Option<EngineEventIngressSink>,
    controller: RefCell<Option<Weak<WKWebExtensionController>>>,
    pool: Rc<CompatibilityBrokerPool>,
    next_request: Cell<Option<u64>>,
    pending: RefCell<HashMap<ExtensionCompatibilityBrokerRequestId, PendingRequest>>,
    sealed: Cell<bool>,
}

impl CompatibilityBroker {
    pub(super) fn new(
        profile: ProfileId,
        sink: Option<EngineEventIngressSink>,
        pool: Rc<CompatibilityBrokerPool>,
    ) -> Rc<Self> {
        Rc::new(Self {
            profile,
            sink,
            controller: RefCell::new(None),
            pool,
            next_request: Cell::new(Some(1)),
            pending: RefCell::new(HashMap::new()),
            sealed: Cell::new(false),
        })
    }

    pub(super) fn bind_controller(&self, controller: &Retained<WKWebExtensionController>) {
        *self.controller.borrow_mut() = Some(Weak::from_retained(controller));
    }

    pub(super) fn begin(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        message: &AnyObject,
        reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
    ) {
        if self.sealed.get() || !self.accepts(controller, context) {
            complete_rejected(
                reply.copy(),
                ExtensionCompatibilityBrokerRejection::InvalidContext,
            );
            return;
        }
        let Some(message) = message.downcast_ref::<NSString>() else {
            complete_rejected(
                reply.copy(),
                ExtensionCompatibilityBrokerRejection::InvalidRequest,
            );
            return;
        };
        let operation = objc2::rc::autoreleasepool(|pool| {
            if message.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                > zephium_core::extensions::MAX_EXTENSION_COMPATIBILITY_BROKER_REQUEST_BYTES
            {
                return Err(());
            }
            ExtensionCompatibilityBrokerOperation::parse_wire(unsafe { message.to_str(pool) })
                .map_err(drop)
        });
        let Ok(operation) = operation else {
            complete_rejected(
                reply.copy(),
                ExtensionCompatibilityBrokerRejection::InvalidRequest,
            );
            return;
        };
        if self.sink.is_none()
            || self.pending.borrow().len()
                >= MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS_PER_PROFILE
            || !self.pool.try_reserve()
        {
            complete_rejected(
                reply.copy(),
                if self.sink.is_none() {
                    ExtensionCompatibilityBrokerRejection::Unsupported
                } else {
                    ExtensionCompatibilityBrokerRejection::CapacityExceeded
                },
            );
            return;
        }
        let Some(id) = self.allocate_request_id() else {
            self.pool.release();
            complete_rejected(
                reply.copy(),
                ExtensionCompatibilityBrokerRejection::CapacityExceeded,
            );
            return;
        };
        let Some(context) = retain_weak(context) else {
            self.pool.release();
            complete_rejected(
                reply.copy(),
                ExtensionCompatibilityBrokerRejection::InvalidContext,
            );
            return;
        };
        let profile = self.profile;
        let Some(watchdog) =
            crate::platform::imp::schedule_content_policy_timeout(REQUEST_TIMEOUT, move || {
                let _ = crate::host::with_extension_browser_request_terminal(move |host| {
                    host.timeout_extension_compatibility_broker_request(profile, id);
                });
            })
        else {
            self.pool.release();
            complete_rejected(
                reply.copy(),
                ExtensionCompatibilityBrokerRejection::BackendUnavailable,
            );
            return;
        };
        self.pending.borrow_mut().insert(
            id,
            PendingRequest {
                context,
                operation,
                reply: reply.copy(),
                watchdog,
                dispatched: false,
                runtime: None,
            },
        );
        if !crate::host::with_extension_browser_request_terminal(move |host| {
            host.finalize_extension_compatibility_broker_request(profile, id);
        }) {
            self.reject_exact(
                id,
                ExtensionCompatibilityBrokerRejection::BackendUnavailable,
            );
        }
    }

    pub(super) fn reject(
        &self,
        reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
        reason: ExtensionCompatibilityBrokerRejection,
    ) {
        complete_rejected(reply.copy(), reason);
    }

    fn accepts(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
    ) -> bool {
        let Some(expected) = self.controller.borrow().as_ref().and_then(Weak::load) else {
            return false;
        };
        std::ptr::eq(&*expected, controller)
            && unsafe { expected.extensionContexts() }.containsObject(context)
    }

    fn allocate_request_id(&self) -> Option<ExtensionCompatibilityBrokerRequestId> {
        for _ in 0..=MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS_PER_PROFILE {
            let candidate = self.next_request.get()?;
            self.next_request.set(candidate.checked_add(1));
            let candidate = ExtensionCompatibilityBrokerRequestId::new(candidate)?;
            if !self.pending.borrow().contains_key(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    pub(super) fn pending_context_identity(
        &self,
        id: ExtensionCompatibilityBrokerRequestId,
    ) -> Option<*const WKWebExtensionContext> {
        self.pending
            .borrow()
            .get(&id)?
            .context
            .load()
            .map(|context| Retained::as_ptr(&context))
    }

    pub(super) fn pending_operation(
        &self,
        id: ExtensionCompatibilityBrokerRequestId,
    ) -> Option<ExtensionCompatibilityBrokerOperation> {
        Some(self.pending.borrow().get(&id)?.operation.clone())
    }

    pub(super) fn finalize(
        &self,
        id: ExtensionCompatibilityBrokerRequestId,
        witness: Option<ExtensionCompatibilityBrokerWitness>,
    ) -> bool {
        let Some(witness) = witness else {
            return self.reject_exact(id, ExtensionCompatibilityBrokerRejection::Unauthorized);
        };
        let request = {
            let mut pending_map = self.pending.borrow_mut();
            let Some(pending) = pending_map.get_mut(&id) else {
                return false;
            };
            if pending.dispatched {
                return true;
            }
            let context_is_current = pending.context.load().is_some_and(|context| {
                self.controller
                    .borrow()
                    .as_ref()
                    .and_then(Weak::load)
                    .is_some_and(|controller| unsafe {
                        controller.extensionContexts().containsObject(&context)
                    })
            });
            if !context_is_current {
                Err(ExtensionCompatibilityBrokerRejection::InvalidContext)
            } else {
                match ExtensionCompatibilityBrokerRequest::authorize(
                    id,
                    pending.operation.clone(),
                    witness,
                ) {
                    Ok(request) => {
                        pending.dispatched = true;
                        pending.runtime = Some(request.runtime());
                        Ok(request)
                    }
                    Err(_) => Err(ExtensionCompatibilityBrokerRejection::Unauthorized),
                }
            }
        };
        let request = match request {
            Ok(request) => request,
            Err(reason) => return self.reject_exact(id, reason),
        };
        let Some(sink) = self.sink.as_ref() else {
            return self.reject_exact(id, ExtensionCompatibilityBrokerRejection::Unsupported);
        };
        let delivered = std::panic::catch_unwind(AssertUnwindSafe(|| {
            sink(EngineEventIngress::global(
                EngineEvent::ExtensionCompatibilityBrokerRequested {
                    request: Box::new(request),
                },
            ));
        }))
        .is_ok();
        delivered
            || self.reject_exact(
                id,
                ExtensionCompatibilityBrokerRejection::BackendUnavailable,
            )
    }

    pub(super) fn settle(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        id: ExtensionCompatibilityBrokerRequestId,
        settlement: ExtensionCompatibilityBrokerSettlement,
        open_options_page: impl FnOnce(&WKWebExtensionContext) -> bool,
    ) -> CompatibilityBrokerSettlementOutcome {
        let Some(pending) = self.take(id) else {
            return CompatibilityBrokerSettlementOutcome::Stale;
        };
        drop(pending.watchdog);
        if !pending.dispatched || pending.runtime != Some(runtime) {
            complete_rejected(
                pending.reply,
                ExtensionCompatibilityBrokerRejection::BackendUnavailable,
            );
            return CompatibilityBrokerSettlementOutcome::IntegrityFailed;
        }
        let response = match settlement {
            ExtensionCompatibilityBrokerSettlement::Applied(
                ExtensionCompatibilityBrokerResult::OptionsPageOpenAuthorized,
            ) if matches!(
                &pending.operation,
                ExtensionCompatibilityBrokerOperation::OpenOptionsPage
            ) =>
            {
                let opened = pending
                    .context
                    .load()
                    .is_some_and(|context| open_options_page(&context));
                Ok(format!(
                    "{{\"v\":1,\"opened\":{}}}",
                    if opened { "true" } else { "false" }
                ))
            }
            ExtensionCompatibilityBrokerSettlement::Applied(result) => encode_result(result),
            ExtensionCompatibilityBrokerSettlement::Rejected(reason) => Err(reason),
        };
        match response {
            Ok(response) if runtime.profile() == self.profile => {
                let response = NSString::from_str(&response);
                pending.reply.call((
                    Retained::as_ptr(&response).cast_mut().cast(),
                    std::ptr::null_mut(),
                ));
                CompatibilityBrokerSettlementOutcome::Settled
            }
            Ok(_) => {
                complete_rejected(
                    pending.reply,
                    ExtensionCompatibilityBrokerRejection::InvalidContext,
                );
                CompatibilityBrokerSettlementOutcome::IntegrityFailed
            }
            Err(reason) => {
                complete_rejected(pending.reply, reason);
                CompatibilityBrokerSettlementOutcome::Settled
            }
        }
    }

    pub(super) fn timeout(&self, id: ExtensionCompatibilityBrokerRequestId) -> bool {
        self.reject_exact(
            id,
            ExtensionCompatibilityBrokerRejection::BackendUnavailable,
        )
    }

    pub(super) fn cancel_context(&self, context: *const WKWebExtensionContext) {
        let ids = self
            .pending
            .borrow()
            .iter()
            .filter_map(|(id, pending)| {
                pending
                    .context
                    .load()
                    .is_some_and(|candidate| Retained::as_ptr(&candidate) == context)
                    .then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.reject_exact(id, ExtensionCompatibilityBrokerRejection::InvalidContext);
        }
    }

    fn reject_exact(
        &self,
        id: ExtensionCompatibilityBrokerRequestId,
        reason: ExtensionCompatibilityBrokerRejection,
    ) -> bool {
        let Some(pending) = self.take(id) else {
            return false;
        };
        drop(pending.watchdog);
        complete_rejected(pending.reply, reason);
        true
    }

    fn take(&self, id: ExtensionCompatibilityBrokerRequestId) -> Option<PendingRequest> {
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
                pending.reply,
                ExtensionCompatibilityBrokerRejection::ShuttingDown,
            );
        }
    }
}

impl Drop for CompatibilityBroker {
    fn drop(&mut self) {
        self.seal_and_reject();
    }
}

fn retain_weak(context: &WKWebExtensionContext) -> Option<Weak<WKWebExtensionContext>> {
    let retained = unsafe {
        Retained::retain(context as *const WKWebExtensionContext as *mut WKWebExtensionContext)
    }?;
    Some(Weak::from_retained(&retained))
}

fn encode_result(
    result: ExtensionCompatibilityBrokerResult,
) -> Result<String, ExtensionCompatibilityBrokerRejection> {
    let entries = match result {
        ExtensionCompatibilityBrokerResult::DefaultSearch { opened } => {
            return Ok(format!(
                "{{\"v\":1,\"opened\":{}}}",
                if opened { "true" } else { "false" }
            ));
        }
        ExtensionCompatibilityBrokerResult::RecentSessionRestore { restored } => {
            return Ok(format!(
                "{{\"v\":1,\"restored\":{}}}",
                if restored { "true" } else { "false" }
            ));
        }
        ExtensionCompatibilityBrokerResult::OptionsPageOpenAuthorized => {
            return Err(ExtensionCompatibilityBrokerRejection::InvalidRequest);
        }
        ExtensionCompatibilityBrokerResult::RecentHistory(entries) => entries,
    };
    if entries.len()
        > usize::from(zephium_core::extensions::MAX_EXTENSION_COMPATIBILITY_HISTORY_RESULTS)
        || entries.iter().any(|entry| {
            !zephium_core::navigation::is_allowed_str(&entry.url)
                || !zephium_core::item::page_title_is_sanitized(&entry.title)
        })
    {
        return Err(ExtensionCompatibilityBrokerRejection::InvalidRequest);
    }
    let mut output = String::from("{\"v\":1,\"items\":[");
    for (index, entry) in entries.iter().enumerate() {
        let url = serde_json::to_string(&entry.url)
            .map_err(|_| ExtensionCompatibilityBrokerRejection::BackendUnavailable)?;
        let title = serde_json::to_string(&entry.title)
            .map_err(|_| ExtensionCompatibilityBrokerRejection::BackendUnavailable)?;
        let separator = usize::from(index != 0);
        let additional = separator
            .checked_add(31)
            .and_then(|bytes| bytes.checked_add(url.len()))
            .and_then(|bytes| bytes.checked_add(title.len()))
            .and_then(|bytes| bytes.checked_add(entry.last_visit.to_string().len()))
            .ok_or(ExtensionCompatibilityBrokerRejection::ResponseTooLarge)?;
        if output.len().saturating_add(additional).saturating_add(2)
            > MAX_EXTENSION_COMPATIBILITY_BROKER_RESPONSE_BYTES
        {
            return Err(ExtensionCompatibilityBrokerRejection::ResponseTooLarge);
        }
        if index != 0 {
            output.push(',');
        }
        output.push_str("{\"url\":");
        output.push_str(&url);
        output.push_str(",\"title\":");
        output.push_str(&title);
        output.push_str(",\"lastVisit\":");
        output.push_str(&entry.last_visit.to_string());
        output.push('}');
    }
    output.push_str("]}");
    if output.len() > MAX_EXTENSION_COMPATIBILITY_BROKER_RESPONSE_BYTES {
        return Err(ExtensionCompatibilityBrokerRejection::ResponseTooLarge);
    }
    Ok(output)
}

fn complete_rejected(reply: Reply, reason: ExtensionCompatibilityBrokerRejection) {
    let error = broker_error(reason);
    reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
}

fn broker_error(reason: ExtensionCompatibilityBrokerRejection) -> Retained<NSError> {
    let code = match reason {
        ExtensionCompatibilityBrokerRejection::InvalidContext => 1,
        ExtensionCompatibilityBrokerRejection::InvalidRequest => 2,
        ExtensionCompatibilityBrokerRejection::Unauthorized => 3,
        ExtensionCompatibilityBrokerRejection::Unsupported => 4,
        ExtensionCompatibilityBrokerRejection::CapacityExceeded => 5,
        ExtensionCompatibilityBrokerRejection::BackendUnavailable => 6,
        ExtensionCompatibilityBrokerRejection::ResponseTooLarge => 7,
        ExtensionCompatibilityBrokerRejection::ShuttingDown => 8,
    };
    let domain = NSString::from_str(ERROR_DOMAIN);
    unsafe { NSError::errorWithDomain_code_userInfo(&domain, code, None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_core::extensions::ExtensionCompatibilityHistoryEntry;

    #[test]
    fn response_encoding_is_exact_and_escapes_untrusted_history_text() {
        let encoded = encode_result(ExtensionCompatibilityBrokerResult::RecentHistory(
            vec![ExtensionCompatibilityHistoryEntry {
                url: "https://example.com/?q=\"x\"".into(),
                title: "quoted \"title\" with \\ slash".into(),
                last_visit: 7,
            }]
            .into_boxed_slice(),
        ))
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["v"], 1);
        assert_eq!(
            parsed["items"][0]["title"],
            "quoted \"title\" with \\ slash"
        );
    }

    #[test]
    fn closed_mutation_results_have_exact_minimal_envelopes() {
        assert_eq!(
            encode_result(ExtensionCompatibilityBrokerResult::DefaultSearch { opened: true }),
            Ok("{\"v\":1,\"opened\":true}".into())
        );
        assert_eq!(
            encode_result(ExtensionCompatibilityBrokerResult::RecentSessionRestore {
                restored: false,
            }),
            Ok("{\"v\":1,\"restored\":false}".into())
        );
    }

    #[test]
    fn response_encoding_rejects_untrusted_or_oversized_results() {
        let invalid_title = ExtensionCompatibilityBrokerResult::RecentHistory(
            vec![ExtensionCompatibilityHistoryEntry {
                url: "https://example.com".into(),
                title: "line\nbreak".into(),
                last_visit: 7,
            }]
            .into_boxed_slice(),
        );
        assert_eq!(
            encode_result(invalid_title),
            Err(ExtensionCompatibilityBrokerRejection::InvalidRequest)
        );

        let too_many = vec![
            ExtensionCompatibilityHistoryEntry {
                url: "https://example.com".into(),
                title: "Title".into(),
                last_visit: 7,
            };
            usize::from(
                zephium_core::extensions::MAX_EXTENSION_COMPATIBILITY_HISTORY_RESULTS
            ) + 1
        ];
        assert_eq!(
            encode_result(ExtensionCompatibilityBrokerResult::RecentHistory(
                too_many.into_boxed_slice()
            )),
            Err(ExtensionCompatibilityBrokerRejection::InvalidRequest)
        );

        let oversized =
            vec![
                ExtensionCompatibilityHistoryEntry {
                    url: format!("https://example.com/?q={}", "a".repeat(1_000)),
                    title: "Title".into(),
                    last_visit: 7,
                };
                usize::from(zephium_core::extensions::MAX_EXTENSION_COMPATIBILITY_HISTORY_RESULTS)
            ];
        assert_eq!(
            encode_result(ExtensionCompatibilityBrokerResult::RecentHistory(
                oversized.into_boxed_slice()
            )),
            Err(ExtensionCompatibilityBrokerRejection::ResponseTooLarge)
        );
    }

    #[test]
    fn process_pool_is_exactly_bounded_and_releasable() {
        let pool = CompatibilityBrokerPool::new();
        for expected in 1..=MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS {
            assert!(pool.try_reserve());
            assert_eq!(pool.pending(), expected);
        }
        assert!(!pool.try_reserve());
        for expected in (0..MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS).rev() {
            pool.release();
            assert_eq!(pool.pending(), expected);
        }
    }
}
