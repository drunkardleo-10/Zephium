//! Bounded, exactly-once WebExtension optional-grant callback ownership.
//!
//! WebKit reports API permissions and host match patterns through independent
//! delegate callbacks without a shared request id. The broker admits at most
//! one unsettled cohort per native extension context, retains each callback,
//! and closes a short collection window before asking the Shell for one
//! all-or-nothing decision. It owns no durable or user-consent authority.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2_foundation::{MainThreadMarker, NSDate, NSSet, NSString};
use objc2_web_kit::{
    WKWebExtensionContext, WKWebExtensionController, WKWebExtensionMatchPattern,
    WKWebExtensionPermission,
};
use zephium_core::extensions::{
    ApiPermissionName, ExtensionNativeOwnershipKey, ExtensionRuntimeInstance,
    MAX_EXTENSION_API_PERMISSIONS, MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
};
use zephium_core::ids::ProfileId;
use zephium_core::injection::MatchPattern;
use zephium_core::ports::engine::EngineEvent;
use zephium_core::ports::extensions::{
    ExtensionRuntimeGrantPrompt, ExtensionRuntimeGrantPromptSettlement,
    ExtensionRuntimeGrantRequest, ExtensionRuntimeGrantRequestId,
    MAX_PENDING_EXTENSION_RUNTIME_GRANT_REQUESTS,
};

use crate::{EngineEventIngress, EngineEventIngressSink};

// The second half of a combined request is observed synchronously on current
// WebKit when the first completion is retained. A short delayed close also
// supports API-only or host-only requests without introducing a visible pause.
const COHORT_COLLECTION_WINDOW: Duration = Duration::from_millis(25);
const RUNTIME_GRANT_PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

type PermissionCompletion = RcBlock<dyn Fn(NonNull<NSSet<WKWebExtensionPermission>>, *mut NSDate)>;
type PatternCompletion = RcBlock<dyn Fn(NonNull<NSSet<WKWebExtensionMatchPattern>>, *mut NSDate)>;

pub(super) struct RuntimeGrantRequestPool {
    pending: Cell<usize>,
}

impl RuntimeGrantRequestPool {
    pub(super) const fn new() -> Self {
        Self {
            pending: Cell::new(0),
        }
    }

    fn try_reserve(&self) -> bool {
        let pending = self.pending.get();
        if pending >= MAX_PENDING_EXTENSION_RUNTIME_GRANT_REQUESTS {
            return false;
        }
        self.pending.set(pending + 1);
        true
    }

    fn release(&self) {
        debug_assert!(self.pending.get() > 0);
        self.pending.set(self.pending.get().saturating_sub(1));
    }

    #[cfg(test)]
    pub(super) fn pending(&self) -> usize {
        self.pending.get()
    }
}

struct DeferredPermissions {
    requested: Retained<NSSet<WKWebExtensionPermission>>,
    parsed: Vec<ApiPermissionName>,
    completion: PermissionCompletion,
}

struct DeferredPatterns {
    response: Retained<NSSet<WKWebExtensionMatchPattern>>,
    parsed: Vec<MatchPattern>,
    completion: PatternCompletion,
}

struct PendingCohort {
    context: Weak<WKWebExtensionContext>,
    runtime: Option<ExtensionRuntimeInstance>,
    permissions: Option<DeferredPermissions>,
    patterns: Option<DeferredPatterns>,
    collection: Option<crate::platform::imp::ContentPolicyTimeout>,
    watchdog: crate::platform::imp::ContentPolicyTimeout,
    dispatched: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimeGrantSettlementOutcome {
    Settled,
    Stale,
    IntegrityFailed,
}

/// One profile controller's optional-grant broker.
pub(super) struct RuntimeGrantRequestBroker {
    profile: ProfileId,
    sink: Option<EngineEventIngressSink>,
    controller: RefCell<Option<Weak<WKWebExtensionController>>>,
    pool: Rc<RuntimeGrantRequestPool>,
    next_request: Cell<Option<u64>>,
    pending: RefCell<HashMap<ExtensionRuntimeGrantRequestId, PendingCohort>>,
    by_context: RefCell<HashMap<usize, ExtensionRuntimeGrantRequestId>>,
    sealed: Cell<bool>,
}

impl RuntimeGrantRequestBroker {
    pub(super) fn new(
        profile: ProfileId,
        sink: Option<EngineEventIngressSink>,
        pool: Rc<RuntimeGrantRequestPool>,
    ) -> Rc<Self> {
        Rc::new(Self {
            profile,
            sink,
            controller: RefCell::new(None),
            pool,
            next_request: Cell::new(Some(1)),
            pending: RefCell::new(HashMap::new()),
            by_context: RefCell::new(HashMap::new()),
            sealed: Cell::new(false),
        })
    }

    pub(super) fn bind_controller(&self, controller: &Retained<WKWebExtensionController>) {
        *self.controller.borrow_mut() = Some(Weak::from_retained(controller));
    }

    pub(super) fn accepts(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
    ) -> bool {
        if self.sealed.get() {
            return false;
        }
        let Some(expected) = self.controller.borrow().as_ref().and_then(Weak::load) else {
            return false;
        };
        if !std::ptr::eq(&*expected, controller) {
            return false;
        }
        // SAFETY: controller, context, and broker are main-thread-owned. The
        // membership read binds this callback to this exact profile owner.
        unsafe { expected.extensionContexts() }.containsObject(context)
    }

    pub(super) fn begin_permissions(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        permissions: &NSSet<WKWebExtensionPermission>,
        completion: &block2::DynBlock<
            dyn Fn(NonNull<NSSet<WKWebExtensionPermission>>, *mut NSDate),
        >,
    ) {
        if !self.accepts(controller, context) {
            deny_permissions(completion);
            return;
        }
        let Ok(parsed) = parse_permissions(permissions) else {
            deny_permissions(completion);
            return;
        };
        if super::grants::validate_runtime_api_permission_request(&parsed).is_err() {
            deny_permissions(completion);
            return;
        }
        let Some(requested) = retain_set(permissions) else {
            deny_permissions(completion);
            return;
        };
        let component = DeferredPermissions {
            requested,
            parsed,
            completion: completion.copy(),
        };
        self.begin_component(context, Some(component), None);
    }

    pub(super) fn begin_patterns(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        patterns: &NSSet<WKWebExtensionMatchPattern>,
        completion: &block2::DynBlock<
            dyn Fn(NonNull<NSSet<WKWebExtensionMatchPattern>>, *mut NSDate),
        >,
    ) {
        if !self.accepts(controller, context) {
            deny_patterns(completion);
            return;
        }
        let Ok(parsed) = parse_patterns(patterns) else {
            deny_patterns(completion);
            return;
        };
        let Ok(response) = super::grants::compile_runtime_host_permission_response(&parsed) else {
            deny_patterns(completion);
            return;
        };
        let Some(response) = resolve_pattern_response(&response) else {
            deny_patterns(completion);
            return;
        };
        let component = DeferredPatterns {
            response,
            parsed,
            completion: completion.copy(),
        };
        self.begin_component(context, None, Some(component));
    }

    fn begin_component(
        &self,
        context: &WKWebExtensionContext,
        mut permissions: Option<DeferredPermissions>,
        mut patterns: Option<DeferredPatterns>,
    ) {
        let context_identity = context as *const WKWebExtensionContext as usize;
        let existing = self.by_context.borrow().get(&context_identity).copied();
        if let Some(existing) = existing {
            enum ExistingAdmission {
                Ready,
                Waiting,
                Reject,
                IntegrityFailed,
            }
            let admission = {
                let mut pending = self.pending.borrow_mut();
                match pending.get_mut(&existing) {
                    None => ExistingAdmission::IntegrityFailed,
                    Some(cohort)
                        if cohort.dispatched
                            || (permissions.is_some() && cohort.permissions.is_some())
                            || (patterns.is_some() && cohort.patterns.is_some()) =>
                    {
                        ExistingAdmission::Reject
                    }
                    Some(cohort) => {
                        if let Some(permissions) = permissions.take() {
                            cohort.permissions = Some(permissions);
                        }
                        if let Some(patterns) = patterns.take() {
                            cohort.patterns = Some(patterns);
                        }
                        if cohort.permissions.is_some() && cohort.patterns.is_some() {
                            // The synchronous peer callback is now the sole
                            // finalization trigger. Cancel the delayed
                            // single-component close before queuing host work,
                            // otherwise a long AppKit re-entry could enqueue a
                            // second terminal for the same request and violate
                            // the fixed three-terminals-per-cohort proof.
                            cohort.collection.take();
                            ExistingAdmission::Ready
                        } else {
                            ExistingAdmission::Waiting
                        }
                    }
                }
            };
            match admission {
                ExistingAdmission::Ready => {
                    if !self.queue_finalize(existing) {
                        self.reject_exact(existing);
                    }
                }
                ExistingAdmission::Waiting => {}
                ExistingAdmission::Reject => complete_components_denied(permissions, patterns),
                ExistingAdmission::IntegrityFailed => {
                    self.reject_missing_existing_component(permissions, patterns)
                }
            }
            return;
        }

        if self.pending.borrow().len() >= MAX_PENDING_EXTENSION_RUNTIME_GRANT_REQUESTS
            || !self.pool.try_reserve()
        {
            complete_components_denied(permissions, patterns);
            return;
        }
        let Some(id) = self.allocate_request_id() else {
            self.pool.release();
            complete_components_denied(permissions, patterns);
            return;
        };
        let Some(context) = retain_weak(context) else {
            self.pool.release();
            complete_components_denied(permissions, patterns);
            return;
        };
        let profile = self.profile;
        let Some(collection) = crate::platform::imp::schedule_content_policy_timeout(
            COHORT_COLLECTION_WINDOW,
            move || {
                let _ = crate::host::with_extension_runtime_grant_terminal(move |host| {
                    host.finalize_extension_runtime_grant_prompt(profile, id);
                });
            },
        ) else {
            self.pool.release();
            complete_components_denied(permissions, patterns);
            return;
        };
        let Some(watchdog) = crate::platform::imp::schedule_content_policy_timeout(
            RUNTIME_GRANT_PROMPT_TIMEOUT,
            move || {
                let _ = crate::host::with_extension_runtime_grant_terminal(move |host| {
                    host.timeout_extension_runtime_grant_prompt(profile, id);
                });
            },
        ) else {
            drop(collection);
            self.pool.release();
            complete_components_denied(permissions, patterns);
            return;
        };
        let previous = self.pending.borrow_mut().insert(
            id,
            PendingCohort {
                context,
                runtime: None,
                permissions,
                patterns,
                collection: Some(collection),
                watchdog,
                dispatched: false,
            },
        );
        let context_previous = self.by_context.borrow_mut().insert(context_identity, id);
        if let Some(previous) = previous {
            self.fail_integrity();
            complete_cohort(previous, false);
            self.pool.release();
        }
        if context_previous.is_some() {
            self.fail_integrity();
        }
        if self.sealed.get() {
            self.reject_exact(id);
        }
    }

    fn reject_missing_existing_component(
        &self,
        permissions: Option<DeferredPermissions>,
        patterns: Option<DeferredPatterns>,
    ) {
        self.fail_integrity();
        complete_components_denied(permissions, patterns);
    }

    fn allocate_request_id(&self) -> Option<ExtensionRuntimeGrantRequestId> {
        let candidate = self.next_request.get()?;
        self.next_request.set(candidate.checked_add(1));
        ExtensionRuntimeGrantRequestId::new(candidate)
    }

    fn queue_finalize(&self, id: ExtensionRuntimeGrantRequestId) -> bool {
        let profile = self.profile;
        crate::host::with_extension_runtime_grant_terminal(move |host| {
            host.finalize_extension_runtime_grant_prompt(profile, id);
        })
    }

    /// Returns the weakly retained native context identity for host-side
    /// published-runtime resolution. The pointer is comparison-only.
    pub(super) fn pending_context_identity(
        &self,
        id: ExtensionRuntimeGrantRequestId,
    ) -> Option<*const WKWebExtensionContext> {
        self.pending
            .borrow()
            .get(&id)?
            .context
            .load()
            .map(|context| Retained::as_ptr(&context))
    }

    /// Closes collection and emits one complete request after the host has
    /// matched the native context to exactly one published runtime owner.
    pub(super) fn finalize(
        &self,
        id: ExtensionRuntimeGrantRequestId,
        subject: Option<(
            ExtensionRuntimeInstance,
            ExtensionNativeOwnershipKey,
            String,
        )>,
    ) -> bool {
        let Some((runtime, key, extension_name)) = subject else {
            return self.reject_exact(id);
        };
        if runtime.profile() != self.profile
            || key.profile() != self.profile
            || key.install_id() != runtime.install_id()
        {
            self.fail_integrity();
            return self.reject_exact(id);
        }
        let request = {
            let mut pending = self.pending.borrow_mut();
            let Some(cohort) = pending.get_mut(&id) else {
                return false;
            };
            if cohort.dispatched {
                return true;
            }
            let Some(context) = cohort.context.load() else {
                drop(pending);
                return self.reject_exact(id);
            };
            let Some(controller) = self.controller.borrow().as_ref().and_then(Weak::load) else {
                drop(pending);
                return self.reject_exact(id);
            };
            if !unsafe { controller.extensionContexts() }.containsObject(&context) {
                drop(pending);
                return self.reject_exact(id);
            }
            let api = cohort
                .permissions
                .as_ref()
                .map(|component| component.parsed.clone())
                .unwrap_or_default();
            let hosts = cohort
                .patterns
                .as_ref()
                .map(|component| component.parsed.clone())
                .unwrap_or_default();
            let Ok(request) = ExtensionRuntimeGrantRequest::new(api, hosts) else {
                drop(pending);
                return self.reject_exact(id);
            };
            cohort.collection.take();
            cohort.runtime = Some(runtime);
            cohort.dispatched = true;
            request
        };

        let Some(sink) = self.sink.as_ref() else {
            return self.reject_exact(id);
        };
        let Ok(prompt) =
            ExtensionRuntimeGrantPrompt::new(id, runtime, key, extension_name, request)
        else {
            return self.reject_exact(id);
        };
        let delivered = std::panic::catch_unwind(AssertUnwindSafe(|| {
            sink(EngineEventIngress::global(
                EngineEvent::ExtensionRuntimeGrantRequested {
                    prompt: Box::new(prompt),
                },
            ));
        }))
        .is_ok();
        delivered || self.reject_exact(id)
    }

    pub(super) fn settle(
        &self,
        runtime: ExtensionRuntimeInstance,
        id: ExtensionRuntimeGrantRequestId,
        settlement: ExtensionRuntimeGrantPromptSettlement,
    ) -> RuntimeGrantSettlementOutcome {
        let Some(cohort) = self.take(id) else {
            return RuntimeGrantSettlementOutcome::Stale;
        };
        if !cohort.dispatched || cohort.runtime != Some(runtime) {
            complete_cohort(cohort, false);
            return RuntimeGrantSettlementOutcome::IntegrityFailed;
        }
        let context_is_current = cohort.context.load().is_some_and(|context| {
            self.controller
                .borrow()
                .as_ref()
                .and_then(Weak::load)
                .is_some_and(|controller| unsafe {
                    controller.extensionContexts().containsObject(&context)
                })
        });
        let granted = settlement == ExtensionRuntimeGrantPromptSettlement::Granted
            && context_is_current
            && !self.sealed.get();
        complete_cohort(cohort, granted);
        RuntimeGrantSettlementOutcome::Settled
    }

    pub(super) fn timeout(&self, id: ExtensionRuntimeGrantRequestId) -> bool {
        self.cancel_exact(id)
    }

    pub(super) fn cancel_context(&self, context: *const WKWebExtensionContext) {
        let id = self.by_context.borrow().get(&(context as usize)).copied();
        if let Some(id) = id {
            self.cancel_exact(id);
        }
    }

    pub(super) fn seal_and_reject(&self) {
        self.sealed.set(true);
        let ids = self.pending.borrow().keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.cancel_exact(id);
        }
        self.controller.borrow_mut().take();
    }

    fn reject_exact(&self, id: ExtensionRuntimeGrantRequestId) -> bool {
        let Some(cohort) = self.take(id) else {
            return false;
        };
        complete_cohort(cohort, false);
        true
    }

    fn cancel_exact(&self, id: ExtensionRuntimeGrantRequestId) -> bool {
        let Some(cohort) = self.take(id) else {
            return false;
        };
        let cancelled_runtime = cohort.dispatched.then_some(cohort.runtime).flatten();
        complete_cohort(cohort, false);
        if let (Some(runtime), Some(sink)) = (cancelled_runtime, self.sink.as_ref()) {
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
                sink(EngineEventIngress::global(
                    EngineEvent::ExtensionRuntimeGrantCancelled {
                        runtime,
                        request: id,
                    },
                ));
            }));
        }
        true
    }

    fn take(&self, id: ExtensionRuntimeGrantRequestId) -> Option<PendingCohort> {
        let cohort = self.pending.borrow_mut().remove(&id)?;
        if let Some(context) = cohort.context.load() {
            self.by_context
                .borrow_mut()
                .remove(&(Retained::as_ptr(&context) as usize));
        } else {
            self.by_context
                .borrow_mut()
                .retain(|_, request| *request != id);
        }
        self.pool.release();
        Some(cohort)
    }

    fn fail_integrity(&self) {
        self.sealed.set(true);
    }
}

fn retain_weak(context: &WKWebExtensionContext) -> Option<Weak<WKWebExtensionContext>> {
    // SAFETY: WebKit supplied a valid retained-for-call Objective-C object.
    let retained = unsafe {
        Retained::retain(context as *const WKWebExtensionContext as *mut WKWebExtensionContext)
    }?;
    Some(Weak::from_retained(&retained))
}

fn retain_set<T: objc2::Message>(set: &NSSet<T>) -> Option<Retained<NSSet<T>>> {
    // SAFETY: WebKit supplied a valid retained-for-call Objective-C set.
    unsafe { Retained::retain(set as *const NSSet<T> as *mut NSSet<T>) }
}

fn resolve_pattern_response(
    patterns: &[String],
) -> Option<Retained<NSSet<WKWebExtensionMatchPattern>>> {
    let mtm = MainThreadMarker::new()?;
    let mut resolved = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        let pattern = unsafe {
            WKWebExtensionMatchPattern::matchPatternWithString(&NSString::from_str(pattern), mtm)
        }?;
        resolved.push(pattern);
    }
    Some(NSSet::from_retained_slice(&resolved))
}

fn parse_permissions(
    permissions: &NSSet<WKWebExtensionPermission>,
) -> Result<Vec<ApiPermissionName>, ()> {
    if permissions.count() == 0 || permissions.count() > MAX_EXTENSION_API_PERMISSIONS {
        return Err(());
    }
    let objects = permissions.allObjects();
    (0..objects.count())
        .map(|index| ApiPermissionName::parse_exact(&objects.objectAtIndex(index).to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())
}

fn parse_patterns(patterns: &NSSet<WKWebExtensionMatchPattern>) -> Result<Vec<MatchPattern>, ()> {
    if patterns.count() == 0 || patterns.count() > MAX_EXTENSION_HOST_PERMISSION_PATTERNS {
        return Err(());
    }
    let objects = patterns.allObjects();
    (0..objects.count())
        .map(|index| {
            MatchPattern::parse(&unsafe { objects.objectAtIndex(index).string() }.to_string())
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())
}

fn deny_permissions(
    completion: &block2::DynBlock<dyn Fn(NonNull<NSSet<WKWebExtensionPermission>>, *mut NSDate)>,
) {
    let empty = NSSet::<WKWebExtensionPermission>::new();
    completion.call((NonNull::from(&*empty), std::ptr::null_mut()));
}

fn deny_patterns(
    completion: &block2::DynBlock<dyn Fn(NonNull<NSSet<WKWebExtensionMatchPattern>>, *mut NSDate)>,
) {
    let empty = NSSet::<WKWebExtensionMatchPattern>::new();
    completion.call((NonNull::from(&*empty), std::ptr::null_mut()));
}

fn complete_components_denied(
    permissions: Option<DeferredPermissions>,
    patterns: Option<DeferredPatterns>,
) {
    if let Some(component) = permissions {
        let empty = NSSet::<WKWebExtensionPermission>::new();
        component
            .completion
            .call((NonNull::from(&*empty), std::ptr::null_mut()));
    }
    if let Some(component) = patterns {
        let empty = NSSet::<WKWebExtensionMatchPattern>::new();
        component
            .completion
            .call((NonNull::from(&*empty), std::ptr::null_mut()));
    }
}

fn complete_cohort(cohort: PendingCohort, granted: bool) {
    drop(cohort.collection);
    drop(cohort.watchdog);
    if let Some(component) = cohort.permissions {
        if granted {
            component
                .completion
                .call((NonNull::from(&*component.requested), std::ptr::null_mut()));
        } else {
            let empty = NSSet::<WKWebExtensionPermission>::new();
            component
                .completion
                .call((NonNull::from(&*empty), std::ptr::null_mut()));
        }
    }
    if let Some(component) = cohort.patterns {
        if granted {
            component
                .completion
                .call((NonNull::from(&*component.response), std::ptr::null_mut()));
        } else {
            let empty = NSSet::<WKWebExtensionMatchPattern>::new();
            component
                .completion
                .call((NonNull::from(&*empty), std::ptr::null_mut()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSString;

    #[test]
    fn global_pool_is_fixed_and_reusable() {
        let pool = RuntimeGrantRequestPool::new();
        for expected in 1..=MAX_PENDING_EXTENSION_RUNTIME_GRANT_REQUESTS {
            assert!(pool.try_reserve());
            assert_eq!(pool.pending(), expected);
        }
        assert!(!pool.try_reserve());
        pool.release();
        assert!(pool.try_reserve());
        assert_eq!(pool.pending(), MAX_PENDING_EXTENSION_RUNTIME_GRANT_REQUESTS);
    }

    #[test]
    fn native_permission_names_use_the_same_bounded_core_grammar() {
        let valid = NSSet::from_retained_slice(&[
            NSString::from_str("clipboardWrite"),
            NSString::from_str("declarativeNetRequest.withHostAccess"),
        ]);
        let parsed = parse_permissions(&valid).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed
            .iter()
            .any(|permission| permission.as_str() == "clipboardWrite"));

        let invalid = NSSet::from_retained_slice(&[NSString::from_str("tabs\n")]);
        assert_eq!(parse_permissions(&invalid), Err(()));
        assert_eq!(
            parse_permissions(&NSSet::<WKWebExtensionPermission>::new()),
            Err(())
        );
    }
}
