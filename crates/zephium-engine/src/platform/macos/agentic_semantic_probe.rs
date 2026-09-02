//! Release-excluded live qualification for the production semantic adapter.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use objc2::{
    rc::{Retained, Weak},
    MainThreadOnly as _,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSDate, NSPoint, NSRect, NSRunLoop, NSSize};
use objc2_web_kit::{WKWebView, WKWebsiteDataStore};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use zephium_agentic::{
    encode_semantic_runtime_invocation, ContextCapabilities, ContextCapability, ContextId,
    ContextIdentity, ContextKind, ContextNavigationTarget, ContextOperationId,
    ContextOwnedViewport, ContextProfileStorageClass, ContextRegistry, ContextRunId,
    ContextSettlement, FixtureRoute, FixtureServer, FixtureServerError, FrameId,
    SemanticCompleteness, SemanticExpansionKind, SemanticFrameJoin, SemanticFrameTrust,
    SemanticFrameUnsupported, SemanticInvocationId, SemanticNode, SemanticObservationAssembler,
    SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
    SemanticOperationClass, SemanticOrigin, SemanticRole, SemanticRuntimeBudget,
    SemanticRuntimeFault, SemanticRuntimeInvocation, SemanticRuntimePortFailure,
    SemanticRuntimeResultError, SemanticSensitivity, SemanticSnapshot, SemanticSnapshotGeneration,
    SemanticValueSummary,
};
use zephium_core::ids::ProfileId;

use super::{
    AgentNavigationCommit, AgentNavigationTerminal, AgentOwnedView, AgentOwnedViewCallbacks,
    NativeContentPolicy,
};

const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(10);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
const MUTATION_GATE_TIMEOUT: Duration = Duration::from_secs(5);
const MUTATION_APPLY_SETTLE: Duration = Duration::from_millis(250);
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const RUN_LOOP_SLICE: Duration = Duration::from_millis(5);
const MAX_DOCUMENT_LOADING_RETRIES: u16 = 512;

struct ProbeHostView {
    view: Retained<NSView>,
}

impl HasWindowHandle for ProbeHostView {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::from(&*self.view).cast::<c_void>();
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(pointer));
        // SAFETY: the retained content view and its retained window outlive
        // the Wry child created from this borrowed handle.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

#[derive(Default)]
struct CallbackState {
    navigation: RefCell<Option<AgentNavigationTerminal>>,
    renderer_lost: Cell<bool>,
    invariant_failed: Cell<bool>,
    callback_panicked: Cell<bool>,
}

impl CallbackState {
    fn failed(&self) -> bool {
        self.renderer_lost.get() || self.invariant_failed.get() || self.callback_panicked.get()
    }
}

struct NativeStateGuard<'a> {
    app: &'a NSApplication,
    window: &'a NSWindow,
    page: &'a WKWebView,
    failed: Cell<bool>,
}

impl NativeStateGuard<'_> {
    fn sample(&self) {
        if self.window.isVisible()
            || self.window.isKeyWindow()
            || !self.page.isHidden()
            || self.app.isActive()
        {
            self.failed.set(true);
        }
    }

    fn failed(&self) -> bool {
        self.failed.get()
    }
}

struct ProbeRuntime<'a, 'native> {
    callbacks: &'a CallbackState,
    run_loop: &'a NSRunLoop,
    native_guard: &'a NativeStateGuard<'native>,
}

impl ProbeRuntime<'_, '_> {
    fn failed(&self) -> bool {
        self.callbacks.failed() || self.native_guard.failed()
    }

    fn pump(&self) {
        pump_once(self.run_loop, Some(self.native_guard));
    }
}

struct CapturedSnapshot {
    request: SemanticObservationRequest,
    snapshot: SemanticSnapshot,
}

struct PendingTeardown {
    execution: Result<(), &'static str>,
    page: Weak<WKWebView>,
    window: Weak<NSWindow>,
    store: Weak<WKWebsiteDataStore>,
    fixture_failure: Option<&'static str>,
}

pub(crate) fn run() -> Result<(), &'static str> {
    let pending = objc2::rc::autoreleasepool(|_| begin())?;
    finish(pending)
}

fn begin() -> Result<PendingTeardown, &'static str> {
    let mtm = MainThreadMarker::new().ok_or("main_thread")?;
    let server = FixtureServer::start().map_err(|_| "fixture_start")?;
    let profile = ProfileId::generate();
    let store = super::new_ephemeral_data_store().map_err(|_| "profile_construct")?;

    let app = NSApplication::sharedApplication(mtm);
    if app.isActive() {
        return Err("focus_baseline");
    }
    if !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
        return Err("application_policy");
    }
    app.finishLaunching();

    let window = new_window(mtm)?;
    let host = ProbeHostView {
        view: window.contentView().ok_or("window_construct")?,
    };
    let callbacks = Rc::new(CallbackState::default());
    let navigation_callbacks = Rc::clone(&callbacks);
    let renderer_callbacks = Rc::clone(&callbacks);
    let invariant_callbacks = Rc::clone(&callbacks);
    let panic_callbacks = Rc::clone(&callbacks);

    let mut registry = ContextRegistry::new();
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile,
        ContextKind::Owned,
    );
    let capabilities = ContextCapabilities::try_new(
        ContextKind::Owned,
        &[ContextCapability::Observe, ContextCapability::Navigate],
    )
    .map_err(|_| "context_construct")?;
    registry
        .reserve(identity, capabilities)
        .map_err(|_| "context_construct")?;
    let construction = registry
        .begin_context(
            identity.id(),
            ContextOperationId::new(1).ok_or("context_construct")?,
        )
        .map_err(|_| "context_construct")?;

    let mut view = super::build_owned_agent_view(
        &host,
        ContextOwnedViewport::STANDARD,
        profile,
        ContextProfileStorageClass::Ephemeral,
        Some(&store),
        AgentOwnedViewCallbacks::new(
            move |terminal| {
                if navigation_callbacks
                    .navigation
                    .try_borrow_mut()
                    .is_ok_and(|mut slot| {
                        if slot.is_some() {
                            false
                        } else {
                            *slot = Some(terminal);
                            true
                        }
                    })
                {
                    return;
                }
                navigation_callbacks.invariant_failed.set(true);
            },
            || {},
            move || renderer_callbacks.renderer_lost.set(true),
            move || invariant_callbacks.invariant_failed.set(true),
            move || panic_callbacks.callback_panicked.set(true),
        ),
    )
    .map_err(|_| "view_construct")?;
    let policy = super::install_content_policy_on_view(view.view(), &NativeContentPolicy::AllowAll)
        .map_err(|_| "content_policy")?;
    registry
        .settle_construction(identity.id(), construction, ContextSettlement::Applied)
        .map_err(|_| "context_construct")?;

    let page = super::native_webview(view.view());
    let native_guard = NativeStateGuard {
        app: &app,
        window: &window,
        page: &page,
        failed: Cell::new(false),
    };
    native_guard.sample();
    let run_loop = NSRunLoop::mainRunLoop();
    let runtime = ProbeRuntime {
        callbacks: &callbacks,
        run_loop: &run_loop,
        native_guard: &native_guard,
    };
    let mut next_invocation = 1_u64;
    let execution = (|| {
        let first_url = server.url(FixtureRoute::SemanticRuntime);
        let first = navigate(
            &mut view,
            &mut registry,
            identity.id(),
            2,
            &first_url,
            &runtime,
        )?;
        let first_capture = capture_snapshot(
            &view,
            first,
            &first_url,
            SemanticSnapshotGeneration::INITIAL,
            &mut next_invocation,
            &runtime,
        )?;
        verify_first_snapshot(&first_capture.snapshot)?;
        registry
            .acknowledge_observation(identity.id(), first)
            .map_err(|_| "first_observation")?;

        let mutation_url = server.url(FixtureRoute::SemanticRuntimeMutation);
        let mutation = navigate(
            &mut view,
            &mut registry,
            identity.id(),
            3,
            &mutation_url,
            &runtime,
        )?;
        let mutation_capture = capture_snapshot(
            &view,
            mutation,
            &mutation_url,
            SemanticSnapshotGeneration::INITIAL,
            &mut next_invocation,
            &runtime,
        )?;
        verify_mutation_before(&mutation_capture.snapshot)?;
        registry
            .acknowledge_observation(identity.id(), mutation)
            .map_err(|_| "mutation_observation")?;

        let mutation_frame = mutation_capture.snapshot.frame().clone();
        let mutation_generation = mutation_capture.snapshot.generation();
        let transient = mutation_capture
            .snapshot
            .nodes()
            .iter()
            .find(|node| node_name_is(node, "Transient mutation anchor"))
            .map(|node| node.reference())
            .ok_or("mutation_anchor_missing")?;
        let frame_boundaries = mutation_capture
            .snapshot
            .nodes()
            .iter()
            .filter(|node| node.role() == SemanticRole::FrameBoundary)
            .map(|node| node.reference())
            .collect::<Vec<_>>();
        let mut assembler =
            SemanticObservationAssembler::new(mutation_capture.request, mutation_capture.snapshot)
                .map_err(|_| "mutation_assembly")?;
        for boundary in frame_boundaries {
            assembler
                .mark_frame_unsupported(
                    FrameId::MAIN,
                    boundary,
                    SemanticFrameUnsupported::PlatformIsolationUnavailable,
                )
                .map_err(|_| "mutation_assembly")?;
        }
        let observation = assembler.finish().map_err(|_| "mutation_assembly")?;
        let expansion_identity = take_semantic_identity(&mut next_invocation)?;
        let expansion = observation
            .begin_expansion(
                SemanticObservationId::new(expansion_identity).ok_or("mutation_identity")?,
                transient,
                &mutation_frame,
                SemanticExpansionKind::Subtree,
                SemanticObservationBudget::INITIAL_FILTERED,
            )
            .map_err(|_| "mutation_expansion")?;
        let expansion_generation = mutation_generation.next().ok_or("mutation_identity")?;
        let expansion_invocation = encode_semantic_runtime_invocation(
            &expansion,
            mutation_frame.clone(),
            SemanticInvocationId::new(expansion_identity).ok_or("mutation_identity")?,
            expansion_generation,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .map_err(|_| "mutation_encode")?;
        wait_for_mutation_gate(&server, &runtime)?;
        if !server.release_semantic_mutation() {
            return Err("mutation_release");
        }
        wait_for_mutation_application(&server, &runtime)?;
        let expansion_deadline = Instant::now()
            .checked_add(SNAPSHOT_TIMEOUT)
            .ok_or("mutation_timeout")?;
        let expansion_outcome =
            dispatch_invocation(&view, expansion_invocation, &runtime, expansion_deadline)?;
        if !matches!(
            expansion_outcome,
            Err(SemanticRuntimePortFailure::Result(
                SemanticRuntimeResultError::Runtime(SemanticRuntimeFault::AnchorMissing)
            ))
        ) {
            return Err("mutation_stale_anchor");
        }
        let post_mutation_generation = expansion_generation.next().ok_or("mutation_identity")?;
        let post_mutation = capture_snapshot(
            &view,
            mutation,
            &mutation_url,
            post_mutation_generation,
            &mut next_invocation,
            &runtime,
        )?;
        verify_mutation_after(&post_mutation.snapshot)?;
        registry
            .acknowledge_observation(identity.id(), mutation)
            .map_err(|_| "post_mutation_observation")?;

        let replacement_url = server.url(FixtureRoute::SemanticRuntimeReplacement);
        let replacement = navigate(
            &mut view,
            &mut registry,
            identity.id(),
            4,
            &replacement_url,
            &runtime,
        )?;
        let replacement_capture = capture_snapshot(
            &view,
            replacement,
            &replacement_url,
            SemanticSnapshotGeneration::INITIAL,
            &mut next_invocation,
            &runtime,
        )?;
        verify_replacement_snapshot(&replacement_capture.snapshot)?;
        registry
            .acknowledge_observation(identity.id(), replacement)
            .map_err(|_| "replacement_observation")?;

        if callbacks.failed()
            || native_guard.failed()
            || view.semantic_pending_for_audit() != Some(false)
            || view
                .attest(profile, ContextProfileStorageClass::Ephemeral, Some(&store))
                .is_err()
            || !server.semantic_mutation_completed()
            || !server.is_healthy()
        {
            return Err("verification");
        }
        Ok(())
    })();

    // Stop the fixed listener before cancelling the native page so an
    // in-flight favicon or subresource socket is cancelled under the fixture's
    // explicit stop state rather than misclassified as a worker fault.
    let fixture_failure = server.shutdown().err().map(fixture_failure_stage);
    super::stop_loading(view.view());
    let policy_clean = policy.retire().is_ok();
    let semantic_clean = view.retire_semantic_runtime();
    let page_weak = Weak::from_retained(&page);
    let window_weak = Weak::from_retained(&window);
    let store_weak = Weak::from_retained(&store);
    drop(page);
    drop(view);
    window.close();
    drop(host);
    drop(window);
    drop(store);
    drop(run_loop);
    drop(registry);

    let execution = if policy_clean && semantic_clean {
        execution
    } else {
        Err("runtime_retire")
    };
    Ok(PendingTeardown {
        execution,
        page: page_weak,
        window: window_weak,
        store: store_weak,
        fixture_failure,
    })
}

fn finish(pending: PendingTeardown) -> Result<(), &'static str> {
    let run_loop = NSRunLoop::mainRunLoop();
    let deadline = Instant::now()
        .checked_add(TEARDOWN_TIMEOUT)
        .ok_or("native_teardown")?;
    while !native_owners_drained(&pending) && Instant::now() < deadline {
        pump_once(&run_loop, None);
    }
    let owners_drained = native_owners_drained(&pending);
    if let Some(failure) = pending.fixture_failure {
        return Err(failure);
    }
    if !owners_drained {
        return Err("native_teardown");
    }
    pending.execution
}

fn fixture_failure_stage(failure: FixtureServerError) -> &'static str {
    match failure {
        FixtureServerError::Io(_) => "fixture_start_io",
        FixtureServerError::NonLoopbackBind => "fixture_bind",
        FixtureServerError::WorkerFailed => "fixture_worker",
        FixtureServerError::NonLoopbackPeer => "fixture_peer",
        FixtureServerError::RequestBudgetExhausted => "fixture_budget",
        FixtureServerError::SemanticMutationInvariant => "fixture_mutation_gate",
        FixtureServerError::SemanticLocationInvariant => "fixture_location_gate",
        FixtureServerError::AcceptFailed => "fixture_accept",
        FixtureServerError::WorkerPanicked => "fixture_panic",
    }
}

fn native_owners_drained(pending: &PendingTeardown) -> bool {
    pending.page.load().is_none()
        && pending.window.load().is_none()
        && pending.store.load().is_none()
}

fn navigate(
    view: &mut AgentOwnedView,
    registry: &mut ContextRegistry,
    id: ContextId,
    operation_id: u64,
    url: &str,
    runtime: &ProbeRuntime<'_, '_>,
) -> Result<zephium_agentic::ContextJoin, &'static str> {
    if runtime.failed() || runtime.callbacks.navigation.borrow().is_some() {
        return Err("navigation_state");
    }
    let operation = registry
        .begin_navigation(
            id,
            ContextOperationId::new(operation_id).ok_or("navigation_state")?,
        )
        .map_err(|_| "navigation_state")?;
    let target = ContextNavigationTarget::parse(url).map_err(|_| "navigation_target")?;
    if view.prepare_semantic_document_load().is_err() {
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err("semantic_epoch");
    }
    let terminal_claimed = Arc::new(AtomicBool::new(false));
    if view
        .navigation()
        .arm(operation, target.clone(), Arc::clone(&terminal_claimed))
        .is_err()
    {
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err("navigation_arm");
    }
    if view.view().load_url(url).is_err() {
        let _ = view.navigation().disarm(operation);
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err("navigation_load");
    }

    let deadline = Instant::now()
        .checked_add(NAVIGATION_TIMEOUT)
        .ok_or("navigation_timeout")?;
    while !runtime.failed()
        && runtime.callbacks.navigation.borrow().is_none()
        && Instant::now() < deadline
    {
        runtime.pump();
    }
    let terminal = runtime
        .callbacks
        .navigation
        .try_borrow_mut()
        .map_err(|_| "navigation_state")?
        .take();
    let disarmed = view.navigation().disarm(operation);
    let applied = terminal.is_some_and(|terminal| {
        terminal.operation() == operation
            && matches!(
                terminal.into_outcome(),
                Ok(AgentNavigationCommit::Web(committed)) if committed == target
            )
    });
    registry
        .settle_navigation(
            id,
            operation,
            if applied {
                ContextSettlement::Applied
            } else {
                ContextSettlement::Refused
            },
        )
        .map_err(|_| "navigation_settle")?;
    if !applied || !disarmed || runtime.failed() || !terminal_claimed.load(Ordering::Acquire) {
        return Err("navigation_terminal");
    }
    registry.join(id).map_err(|_| "navigation_settle")
}

fn capture_snapshot(
    view: &AgentOwnedView,
    context: zephium_agentic::ContextJoin,
    url: &str,
    first_generation: SemanticSnapshotGeneration,
    next_invocation: &mut u64,
    runtime: &ProbeRuntime<'_, '_>,
) -> Result<CapturedSnapshot, &'static str> {
    let origin = SemanticOrigin::parse(url).map_err(|_| "snapshot_origin")?;
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        context.frame_generation(),
        origin,
        SemanticFrameTrust::SameOrigin,
    )
    .map_err(|_| "snapshot_frame")?;
    let deadline = Instant::now()
        .checked_add(SNAPSHOT_TIMEOUT)
        .ok_or("snapshot_timeout")?;

    for retry in 0..MAX_DOCUMENT_LOADING_RETRIES {
        if runtime.failed() || Instant::now() >= deadline {
            return Err("snapshot_timeout");
        }
        let invocation_value = take_semantic_identity(next_invocation)?;
        let generation = first_generation
            .get()
            .checked_add(u64::from(retry))
            .and_then(SemanticSnapshotGeneration::new)
            .ok_or("snapshot_identity")?;
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(invocation_value).ok_or("snapshot_identity")?,
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame.clone(),
            SemanticInvocationId::new(invocation_value).ok_or("snapshot_identity")?,
            generation,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .map_err(|_| "snapshot_encode")?;
        let outcome = dispatch_invocation(view, invocation, runtime, deadline)?;
        match outcome {
            Ok(snapshot) => return Ok(CapturedSnapshot { request, snapshot }),
            Err(SemanticRuntimePortFailure::Result(SemanticRuntimeResultError::Runtime(
                SemanticRuntimeFault::DocumentLoading,
            ))) => runtime.pump(),
            Err(_) => return Err("snapshot_result"),
        }
    }
    Err("snapshot_timeout")
}

fn dispatch_invocation(
    view: &AgentOwnedView,
    invocation: SemanticRuntimeInvocation,
    runtime: &ProbeRuntime<'_, '_>,
    deadline: Instant,
) -> Result<Result<SemanticSnapshot, SemanticRuntimePortFailure>, &'static str> {
    if runtime.failed() || Instant::now() >= deadline {
        return Err("snapshot_timeout");
    }
    let result = Rc::new(RefCell::new(None));
    let completion_failed = Rc::new(Cell::new(false));
    let completion = Rc::clone(&result);
    let completion_failure = Rc::clone(&completion_failed);
    view.dispatch_semantic(invocation, move |outcome| {
        let Ok(mut slot) = completion.try_borrow_mut() else {
            completion_failure.set(true);
            return;
        };
        if slot.is_some() {
            completion_failure.set(true);
        } else {
            *slot = Some(outcome);
        }
    })
    .map_err(|_| "snapshot_dispatch")?;
    while result.borrow().is_none()
        && !completion_failed.get()
        && !runtime.failed()
        && Instant::now() < deadline
    {
        runtime.pump();
    }
    if completion_failed.get() || runtime.failed() {
        return Err("snapshot_state");
    }
    let outcome = result
        .try_borrow_mut()
        .map_err(|_| "snapshot_state")?
        .take()
        .ok_or("snapshot_timeout")?;
    Ok(outcome)
}

fn take_semantic_identity(next: &mut u64) -> Result<u64, &'static str> {
    let identity = *next;
    if identity == 0 {
        return Err("snapshot_identity");
    }
    *next = identity.checked_add(1).ok_or("snapshot_identity")?;
    Ok(identity)
}

fn wait_for_mutation_gate(
    server: &FixtureServer,
    runtime: &ProbeRuntime<'_, '_>,
) -> Result<(), &'static str> {
    let deadline = Instant::now()
        .checked_add(MUTATION_GATE_TIMEOUT)
        .ok_or("mutation_timeout")?;
    while !server.semantic_mutation_waiting()
        && server.is_healthy()
        && !runtime.failed()
        && Instant::now() < deadline
    {
        runtime.pump();
    }
    if !server.is_healthy() {
        return Err("mutation_fixture");
    }
    if runtime.failed() {
        return Err("mutation_native_state");
    }
    if !server.semantic_mutation_waiting() {
        return Err("mutation_gate_timeout");
    }
    Ok(())
}

fn wait_for_mutation_application(
    server: &FixtureServer,
    runtime: &ProbeRuntime<'_, '_>,
) -> Result<(), &'static str> {
    let deadline = Instant::now()
        .checked_add(MUTATION_GATE_TIMEOUT)
        .ok_or("mutation_timeout")?;
    while !server.semantic_mutation_completed()
        && server.is_healthy()
        && !runtime.failed()
        && Instant::now() < deadline
    {
        runtime.pump();
    }
    if !server.semantic_mutation_completed() || !server.is_healthy() || runtime.failed() {
        return Err("mutation_apply");
    }

    let settle_deadline = Instant::now()
        .checked_add(MUTATION_APPLY_SETTLE)
        .ok_or("mutation_timeout")?;
    while Instant::now() < settle_deadline {
        runtime.pump();
        if runtime.failed() {
            return Err("mutation_apply");
        }
    }
    Ok(())
}

fn verify_first_snapshot(snapshot: &SemanticSnapshot) -> Result<(), &'static str> {
    if snapshot.completeness() != SemanticCompleteness::Complete {
        return Err("first_incomplete");
    }
    for (needle, stage) in [
        ("First semantic epoch", "first_epoch_missing"),
        ("Page bridge absent", "first_bridge_absence_missing"),
        ("Primary semantic action", "first_primary_action_missing"),
    ] {
        if !snapshot_contains(snapshot, needle) {
            return Err(stage);
        }
    }
    for (needle, stage) in [
        (
            "Closed internal must remain absent",
            "first_closed_shadow_exposed",
        ),
        ("Page bridge present", "first_page_bridge_exposed"),
        ("page-world-forgery", "first_page_world_forgery_exposed"),
        ("fixture-password-value", "first_password_exposed"),
        ("Bearer abcdefghijklmnop", "first_token_exposed"),
    ] {
        if snapshot_contains(snapshot, needle) {
            return Err(stage);
        }
    }
    let password = snapshot
        .nodes()
        .iter()
        .find(|node| node.role() == SemanticRole::Password)
        .ok_or("first_password_missing")?;
    let mut token_candidates = snapshot.nodes().iter().filter(|node| {
        node.role() == SemanticRole::Textbox
            && node.value() == Some(&SemanticValueSummary::Redacted)
            && node.sensitivity() == SemanticSensitivity::Secret
    });
    let token = token_candidates.next().ok_or("first_token_missing")?;
    if token_candidates.next().is_some() {
        return Err("first_token_ambiguous");
    }
    if password.value() != Some(&SemanticValueSummary::Redacted) {
        return Err("first_password_not_redacted");
    }
    if password.sensitivity() != SemanticSensitivity::Secret {
        return Err("first_password_not_secret");
    }
    if token.value() != Some(&SemanticValueSummary::Redacted) {
        return Err("first_token_not_redacted");
    }
    if token.sensitivity() != SemanticSensitivity::Secret {
        return Err("first_token_not_secret");
    }
    if !snapshot_contains(snapshot, "Open shadow semantic action") {
        return Err("first_open_shadow_missing");
    }
    if !snapshot.nodes().iter().any(|node| {
        node.role() == SemanticRole::Button
            && node.operations().contains(SemanticOperationClass::Click)
    }) {
        return Err("first_click_operation_missing");
    }
    if !snapshot
        .nodes()
        .iter()
        .any(|node| node.role() == SemanticRole::FrameBoundary)
    {
        return Err("first_frame_boundary_missing");
    }
    Ok(())
}

fn verify_mutation_before(snapshot: &SemanticSnapshot) -> Result<(), &'static str> {
    if snapshot.completeness() != SemanticCompleteness::Complete {
        return Err("mutation_before_incomplete");
    }
    for (needle, stage) in [
        ("Mutation semantic epoch", "mutation_epoch_missing"),
        ("Mutation stable before", "mutation_stable_before_missing"),
        ("Transient mutation anchor", "mutation_anchor_missing"),
    ] {
        if !snapshot_contains(snapshot, needle) {
            return Err(stage);
        }
    }
    let transient = snapshot
        .nodes()
        .iter()
        .find(|node| node_name_is(node, "Transient mutation anchor"))
        .ok_or("mutation_anchor_missing")?;
    if transient.role() != SemanticRole::Button
        || !transient
            .operations()
            .contains(SemanticOperationClass::Click)
    {
        return Err("mutation_anchor_authority");
    }
    if snapshot_contains(snapshot, "Mutation stable after") {
        return Err("mutation_applied_early");
    }
    Ok(())
}

fn verify_mutation_after(snapshot: &SemanticSnapshot) -> Result<(), &'static str> {
    if snapshot.completeness() != SemanticCompleteness::Complete {
        return Err("mutation_after_incomplete");
    }
    for (needle, stage) in [
        ("Mutation semantic epoch", "mutation_epoch_missing"),
        ("Mutation stable after", "mutation_stable_after_missing"),
    ] {
        if !snapshot_contains(snapshot, needle) {
            return Err(stage);
        }
    }
    for (needle, stage) in [
        ("Mutation stable before", "mutation_stale_label_exposed"),
        ("Transient mutation anchor", "mutation_stale_anchor_exposed"),
    ] {
        if snapshot_contains(snapshot, needle) {
            return Err(stage);
        }
    }
    Ok(())
}

fn verify_replacement_snapshot(snapshot: &SemanticSnapshot) -> Result<(), &'static str> {
    if snapshot.completeness() != SemanticCompleteness::Complete {
        return Err("replacement_incomplete");
    }
    for (needle, stage) in [
        ("Replacement semantic epoch", "replacement_epoch_missing"),
        ("Replacement semantic action", "replacement_action_missing"),
        ("Page bridge absent", "replacement_bridge_absence_missing"),
    ] {
        if !snapshot_contains(snapshot, needle) {
            return Err(stage);
        }
    }
    for (needle, stage) in [
        ("First semantic epoch", "replacement_stale_epoch_exposed"),
        (
            "Mutation semantic epoch",
            "replacement_mutation_epoch_exposed",
        ),
        (
            "Mutation stable before",
            "replacement_mutation_stale_exposed",
        ),
        (
            "Mutation stable after",
            "replacement_mutation_fresh_exposed",
        ),
        (
            "Transient mutation anchor",
            "replacement_mutation_anchor_exposed",
        ),
        ("Page bridge present", "replacement_page_bridge_exposed"),
        (
            "replacement-page-world-forgery",
            "replacement_page_world_forgery_exposed",
        ),
    ] {
        if snapshot_contains(snapshot, needle) {
            return Err(stage);
        }
    }
    Ok(())
}

fn node_name_is(node: &SemanticNode, expected: &str) -> bool {
    node.name().is_some_and(|name| name.as_str() == expected)
}

fn snapshot_contains(snapshot: &SemanticSnapshot, needle: &str) -> bool {
    snapshot.nodes().iter().any(|node| {
        node.name()
            .is_some_and(|value| value.as_str().contains(needle))
            || node
                .text()
                .is_some_and(|value| value.as_str().contains(needle))
            || matches!(
                node.value(),
                Some(SemanticValueSummary::Text(value)) if value.as_str().contains(needle)
            )
    })
}

fn new_window(mtm: MainThreadMarker) -> Result<Retained<NSWindow>, &'static str> {
    // SAFETY: the marker proves AppKit main-thread affinity. The window is
    // never ordered front and cannot release itself while the child exists.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(80.0, 80.0), NSSize::new(760.0, 640.0)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.orderOut(None);
    window.contentView().ok_or("window_construct")?;
    Ok(window)
}

fn pump_once(run_loop: &NSRunLoop, native_guard: Option<&NativeStateGuard<'_>>) {
    if let Some(native_guard) = native_guard {
        native_guard.sample();
    }
    objc2::rc::autoreleasepool(|_| {
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(
            RUN_LOOP_SLICE.as_secs_f64(),
        ));
    });
    if let Some(native_guard) = native_guard {
        native_guard.sample();
    }
}
