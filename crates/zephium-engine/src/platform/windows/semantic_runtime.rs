#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

//! Production WebView2 adapter for the fixed semantic isolated world.
//!
//! All CDP syntax lives in the platform-neutral closed protocol module. This
//! adapter contributes only exact COM identity, bounded native string handling,
//! one-command-at-a-time sequencing, document lifecycle cancellation, and
//! callback ownership. Chromium's named document-start worlds grant universal
//! access, so this adapter instead lazily installs the fixed program only after
//! proving a root-frame world created with universal access explicitly denied.

use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use webview2_com::DevToolsProtocolEventReceivedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl,
    ICoreWebView2DevToolsProtocolEventReceiver,
};
use windows_core::{IUnknown, Interface as _, HRESULT, HSTRING, PCWSTR, PWSTR};
use zephium_agentic::{
    SemanticInvocationId, SemanticRuntimeInvocation, SemanticRuntimePortFailure,
    SemanticRuntimeResultError, SemanticSnapshot, MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS,
};

use crate::platform::agent_semantic_cdp_protocol::{
    decode_empty_success, decode_invocation_response, decode_root_frame,
    decode_runtime_install_response, get_frame_tree_command, install_runtime_in_context_command,
    invoke_runtime_command, runtime_disable_command, runtime_enable_command,
    FixedSemanticCdpCommand, SemanticCdpInvocationError, SemanticContextDiscovery,
    SemanticExecutionContext, SemanticWorldName, MAX_CONTEXT_EVENT_BYTES,
};

const MAX_CLEANUP_DISABLE_ATTEMPTS: u8 = 1;
static NEXT_SEMANTIC_WORLD: AtomicU64 = AtomicU64::new(1);

fn next_world_name() -> Result<SemanticWorldName, ()> {
    let epoch = NEXT_SEMANTIC_WORLD
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| ())?;
    let unpredictable = windows_core::GUID::new().map_err(|_| ())?.to_u128();
    SemanticWorldName::from_nonce(epoch, unpredictable).map_err(|_| ())
}

type SemanticCompletion = Box<dyn FnOnce(Result<SemanticSnapshot, SemanticRuntimePortFailure>)>;
type NativeCompletion = Box<dyn FnOnce(Result<String, NativeCdpFailure>)>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeCdpFailure {
    Native,
    ResponseLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentPhase {
    Loading,
    Ready,
    RendererLost,
    Failed,
    Retired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandStage {
    GetFrameTree,
    RuntimeEnable,
    CreateIsolatedWorld,
    RuntimeDisable,
    InstallRuntime,
    Invoke,
    CleanupRuntimeDisable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InFlightCommand {
    document_generation: u64,
    stage: CommandStage,
}

struct PendingInvocation {
    invocation: SemanticRuntimeInvocation,
    completion: SemanticCompletion,
}

struct RuntimeState {
    bound: bool,
    phase: DocumentPhase,
    document_generation: u64,
    world: SemanticWorldName,
    completed_invocations: u16,
    pending: Option<PendingInvocation>,
    in_flight: Option<InFlightCommand>,
    discovery: Option<SemanticContextDiscovery>,
    installed_context: Option<SemanticExecutionContext>,
    context_events_enabled: bool,
    cleanup_disable_pending: bool,
    cleanup_disable_attempts: u8,
}

impl RuntimeState {
    fn new(world: SemanticWorldName) -> Self {
        Self {
            bound: false,
            phase: DocumentPhase::Loading,
            document_generation: 1,
            world,
            completed_invocations: 0,
            pending: None,
            in_flight: None,
            discovery: None,
            installed_context: None,
            context_events_enabled: false,
            cleanup_disable_pending: false,
            cleanup_disable_attempts: 0,
        }
    }
}

struct SharedRuntime {
    state: RefCell<RuntimeState>,
    native: RefCell<Weak<NativeRuntime>>,
}

/// Pre-construction semantic authority. It is created before Wry attaches any
/// navigation handler so the construction-only commit can be lifecycle-bound.
#[derive(Clone)]
pub(crate) struct AgentSemanticRuntimePlan {
    shared: Rc<SharedRuntime>,
}

impl AgentSemanticRuntimePlan {
    pub(crate) fn prepare() -> Result<Self, ()> {
        let world = next_world_name()?;
        Ok(Self {
            shared: Rc::new(SharedRuntime {
                state: RefCell::new(RuntimeState::new(world)),
                native: RefCell::new(Weak::new()),
            }),
        })
    }

    pub(crate) fn document_committed(&self) -> Result<(), ()> {
        if let Some(native) = self.native() {
            return native.document_committed();
        }
        let mut state = self.shared.state.try_borrow_mut().map_err(|_| ())?;
        if state.phase != DocumentPhase::Loading {
            if state.phase != DocumentPhase::Retired {
                state.phase = DocumentPhase::Failed;
            }
            return Err(());
        }
        state.phase = DocumentPhase::Ready;
        Ok(())
    }

    pub(crate) fn renderer_lost(&self) {
        if let Some(native) = self.native() {
            native.renderer_lost();
            return;
        }
        if let Ok(mut state) = self.shared.state.try_borrow_mut() {
            if state.phase != DocumentPhase::Retired {
                state.phase = DocumentPhase::RendererLost;
            }
        }
    }

    pub(crate) fn bind(
        self,
        core: &ICoreWebView2,
        on_invariant_failure: Rc<dyn Fn()>,
        on_callback_panic: Rc<dyn Fn()>,
    ) -> Result<AgentSemanticRuntimeRegistration, ()> {
        {
            let mut state = self.shared.state.try_borrow_mut().map_err(|_| ())?;
            if state.bound {
                return Err(());
            }
            // Claim the one binding permit before the external COM call. A
            // navigation callback may re-enter while the event receiver is
            // registered, but no second controller can bind this plan.
            state.bound = true;
        }
        let native = Rc::new(NativeRuntime {
            core: core.clone(),
            shared: self.shared.clone(),
            on_invariant_failure,
            on_callback_panic,
        });
        let events = match register_context_event(&native) {
            Ok(events) => events,
            Err(_) => {
                if let Ok(mut state) = self.shared.state.try_borrow_mut() {
                    state.bound = false;
                }
                return Err(());
            }
        };
        let stored = self
            .shared
            .native
            .try_borrow_mut()
            .ok()
            .filter(|slot| slot.upgrade().is_none())
            .is_some_and(|mut slot| {
                *slot = Rc::downgrade(&native);
                true
            });
        if !stored {
            if let Ok(mut state) = self.shared.state.try_borrow_mut() {
                state.bound = false;
            }
            return Err(());
        }
        Ok(AgentSemanticRuntimeRegistration {
            controller: AgentSemanticRuntimeController { native },
            events: Some(events),
        })
    }

    fn native(&self) -> Option<Rc<NativeRuntime>> {
        self.shared.native.try_borrow().ok()?.upgrade()
    }
}

impl std::fmt::Debug for AgentSemanticRuntimePlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentSemanticRuntimePlan([redacted])")
    }
}

#[derive(Clone)]
pub(crate) struct AgentSemanticRuntimeController {
    native: Rc<NativeRuntime>,
}

impl AgentSemanticRuntimeController {
    pub(crate) fn dispatch(
        &self,
        invocation: SemanticRuntimeInvocation,
        completion: impl FnOnce(Result<SemanticSnapshot, SemanticRuntimePortFailure>) + 'static,
    ) -> Result<(), SemanticRuntimePortFailure> {
        self.native.dispatch(invocation, Box::new(completion))
    }

    pub(crate) fn begin_document_load(&self) -> Result<(), ()> {
        self.native.begin_document_load()
    }

    pub(crate) fn cancel(&self) {
        self.native.cancel();
    }

    pub(crate) fn timeout(&self, invocation: SemanticInvocationId) -> bool {
        self.native.timeout(invocation)
    }

    pub(crate) fn pending_for_audit(&self) -> Option<bool> {
        self.native.pending_for_audit()
    }

    pub(crate) fn work_drained_for_audit(&self) -> Option<bool> {
        self.native.work_drained_for_audit()
    }

    /// Reports the native lifecycle edge that WebView2 maps from this exact
    /// main-frame `ContentLoading` event. It is content-free and grants no
    /// semantic invocation or screenshot authority by itself.
    pub(crate) fn document_content_available_for_audit(&self) -> Option<bool> {
        let state = self.native.shared.state.try_borrow().ok()?;
        Some(state.bound && state.phase == DocumentPhase::Ready)
    }

    pub(crate) fn attest(&self, core: &ICoreWebView2) -> bool {
        self.native.attest(core)
    }
}

impl std::fmt::Debug for AgentSemanticRuntimeController {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentSemanticRuntimeController([native, redacted])")
    }
}

pub(crate) struct AgentSemanticRuntimeRegistration {
    controller: AgentSemanticRuntimeController,
    events: Option<CdpEventRegistration>,
}

impl AgentSemanticRuntimeRegistration {
    pub(crate) const fn controller(&self) -> &AgentSemanticRuntimeController {
        &self.controller
    }

    /// Retires logical authority and the exact native event callback. No
    /// document-start script registration exists: the fixed runtime is scoped
    /// to the isolated execution context and dies with that document/target.
    pub(crate) fn retire(mut self) -> Result<(), ()> {
        let state_clean = self.controller.native.retire();
        let event_clean = self
            .events
            .take()
            .is_some_and(|mut registration| registration.retire());
        if state_clean && event_clean {
            Ok(())
        } else {
            Err(())
        }
    }
}

impl Drop for AgentSemanticRuntimeRegistration {
    fn drop(&mut self) {
        self.controller.native.retire();
        if let Some(mut events) = self.events.take() {
            let _ = events.retire();
        }
    }
}

struct NativeRuntime {
    core: ICoreWebView2,
    shared: Rc<SharedRuntime>,
    on_invariant_failure: Rc<dyn Fn()>,
    on_callback_panic: Rc<dyn Fn()>,
}

impl NativeRuntime {
    fn dispatch(
        self: &Rc<Self>,
        invocation: SemanticRuntimeInvocation,
        completion: SemanticCompletion,
    ) -> Result<(), SemanticRuntimePortFailure> {
        let refusal = {
            let state = match self.shared.state.try_borrow() {
                Ok(state) => state,
                Err(_) => {
                    self.invoke_completion(completion, Err(SemanticRuntimePortFailure::Transport));
                    self.invariant_failed();
                    return Err(SemanticRuntimePortFailure::Transport);
                }
            };
            if !state.bound || state.phase == DocumentPhase::Loading {
                Some(SemanticRuntimePortFailure::NotReady)
            } else if state.phase == DocumentPhase::RendererLost {
                Some(SemanticRuntimePortFailure::RendererLost)
            } else if state.phase == DocumentPhase::Retired {
                Some(SemanticRuntimePortFailure::Retired)
            } else if state.phase == DocumentPhase::Failed {
                Some(SemanticRuntimePortFailure::Transport)
            } else if state.pending.is_some() || state.in_flight.is_some() {
                Some(SemanticRuntimePortFailure::ResourceExhausted)
            } else if state.completed_invocations >= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS {
                Some(SemanticRuntimePortFailure::InvocationLimit)
            } else {
                None
            }
        };
        if let Some(failure) = refusal {
            self.invoke_completion(completion, Err(failure));
            return Err(failure);
        }

        let (generation, installed_context) = {
            let mut state = match self.shared.state.try_borrow_mut() {
                Ok(state) => state,
                Err(_) => {
                    self.invoke_completion(completion, Err(SemanticRuntimePortFailure::Transport));
                    self.invariant_failed();
                    return Err(SemanticRuntimePortFailure::Transport);
                }
            };
            let generation = state.document_generation;
            state.pending = Some(PendingInvocation {
                invocation,
                completion,
            });
            (generation, state.installed_context.clone())
        };
        let command = match installed_context.as_ref() {
            Some(context) => self.shared.state.try_borrow().ok().and_then(|state| {
                invoke_runtime_command(context, &state.pending.as_ref()?.invocation).ok()
            }),
            None => Some(get_frame_tree_command()),
        };
        let stage = if installed_context.is_some() {
            CommandStage::Invoke
        } else {
            CommandStage::GetFrameTree
        };
        if command
            .ok_or(())
            .and_then(|command| self.dispatch_command(generation, stage, command))
            .is_err()
        {
            self.fail_current(SemanticRuntimePortFailure::Transport, true);
            return Err(SemanticRuntimePortFailure::Transport);
        }
        Ok(())
    }

    fn document_committed(&self) -> Result<(), ()> {
        let mut state = self.shared.state.try_borrow_mut().map_err(|_| ())?;
        if !state.bound || state.phase != DocumentPhase::Loading {
            if state.phase != DocumentPhase::Retired {
                state.phase = DocumentPhase::Failed;
            }
            drop(state);
            self.invariant_failed();
            return Err(());
        }
        state.phase = DocumentPhase::Ready;
        Ok(())
    }

    fn begin_document_load(self: &Rc<Self>) -> Result<(), ()> {
        {
            let state = self.shared.state.try_borrow().map_err(|_| ())?;
            if !state.bound || state.phase == DocumentPhase::Retired {
                return Err(());
            }
        }
        // Blink caches an inspector world by frame and name. A new name per
        // document prevents a cross-origin navigation from reusing the prior
        // world's security origin even if the underlying LocalFrame survives.
        let next_world = next_world_name()?;
        let completion = {
            let mut state = self.shared.state.try_borrow_mut().map_err(|_| ())?;
            if !state.bound || state.phase == DocumentPhase::Retired {
                return Err(());
            }
            state.document_generation = state.document_generation.checked_add(1).ok_or(())?;
            state.world = next_world;
            state.phase = DocumentPhase::Loading;
            state.completed_invocations = 0;
            state.discovery = None;
            state.installed_context = None;
            state.cleanup_disable_pending |= state.context_events_enabled;
            state.pending.take().map(|pending| pending.completion)
        };
        if let Some(completion) = completion {
            self.invoke_completion(
                completion,
                Err(SemanticRuntimePortFailure::DocumentReplaced),
            );
        }
        self.maybe_dispatch_cleanup_disable();
        Ok(())
    }

    fn renderer_lost(self: &Rc<Self>) {
        self.invalidate(
            DocumentPhase::RendererLost,
            SemanticRuntimePortFailure::RendererLost,
        );
    }

    fn cancel(self: &Rc<Self>) {
        self.invalidate(DocumentPhase::Failed, SemanticRuntimePortFailure::Cancelled);
    }

    fn timeout(self: &Rc<Self>, invocation: SemanticInvocationId) -> bool {
        let completion = {
            let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                self.invariant_failed();
                return false;
            };
            if state
                .pending
                .as_ref()
                .is_none_or(|pending| pending.invocation.invocation() != invocation)
            {
                return false;
            }
            state.phase = DocumentPhase::Failed;
            state.discovery = None;
            state.installed_context = None;
            state.cleanup_disable_pending |= state.context_events_enabled;
            state.pending.take().map(|pending| pending.completion)
        };
        if let Some(completion) = completion {
            self.invoke_completion(completion, Err(SemanticRuntimePortFailure::TimedOut));
        }
        self.maybe_dispatch_cleanup_disable();
        true
    }

    fn retire(self: &Rc<Self>) -> bool {
        let (completion, contradictory_retirement) = {
            let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                self.invariant_failed();
                return false;
            };
            let already_retired = state.phase == DocumentPhase::Retired;
            state.phase = DocumentPhase::Retired;
            state.bound = false;
            state.discovery = None;
            state.installed_context = None;
            state.context_events_enabled = false;
            state.cleanup_disable_pending = false;
            state.cleanup_disable_attempts = 0;
            let completion = state.pending.take().map(|pending| pending.completion);
            let contradictory_retirement = already_retired && completion.is_some();
            (completion, contradictory_retirement)
        };
        if let Some(completion) = completion {
            self.invoke_completion(completion, Err(SemanticRuntimePortFailure::Retired));
        }
        if contradictory_retirement {
            self.invariant_failed();
        }
        true
    }

    fn pending_for_audit(&self) -> Option<bool> {
        let state = self.shared.state.try_borrow().ok()?;
        let pending = state.pending.is_some();
        let active_command = state.in_flight.is_some();
        let waiting_for_context =
            pending && !active_command && state.context_events_enabled && state.discovery.is_some();
        let stale_native_work = !pending
            && active_command
            && state.phase != DocumentPhase::Ready
            && state.discovery.is_none();
        let reporting_is_owned = !state.context_events_enabled
            || (pending && state.discovery.is_some())
            || state.cleanup_disable_pending;
        let valid = state.bound
            && state.completed_invocations <= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS
            && state.cleanup_disable_attempts <= MAX_CLEANUP_DISABLE_ATTEMPTS
            && (pending == active_command || waiting_for_context || stale_native_work)
            && reporting_is_owned
            && (!matches!(
                state.phase,
                DocumentPhase::RendererLost | DocumentPhase::Failed | DocumentPhase::Retired
            ) || (!pending && state.discovery.is_none()));
        valid.then_some(pending)
    }

    fn work_drained_for_audit(&self) -> Option<bool> {
        let state = self.shared.state.try_borrow().ok()?;
        let drained = state.pending.is_none()
            && state.in_flight.is_none()
            && state.discovery.is_none()
            && !state.context_events_enabled
            && !state.cleanup_disable_pending;
        let valid = state.bound
            && state.completed_invocations <= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS
            && state.cleanup_disable_attempts <= MAX_CLEANUP_DISABLE_ATTEMPTS
            && state.phase != DocumentPhase::Retired;
        valid.then_some(drained)
    }

    fn attest(self: &Rc<Self>, core: &ICoreWebView2) -> bool {
        same_interface(&self.core, core)
            && self
                .shared
                .native
                .try_borrow()
                .ok()
                .and_then(|native| native.upgrade())
                .is_some_and(|native| Rc::ptr_eq(&native, self))
            && self.pending_for_audit().is_some()
    }

    fn invalidate(self: &Rc<Self>, phase: DocumentPhase, failure: SemanticRuntimePortFailure) {
        let completion = {
            let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                self.invariant_failed();
                return;
            };
            state.phase = phase;
            state.discovery = None;
            state.installed_context = None;
            state.cleanup_disable_pending |= state.context_events_enabled;
            state.pending.take().map(|pending| pending.completion)
        };
        if let Some(completion) = completion {
            self.invoke_completion(completion, Err(failure));
        }
        self.maybe_dispatch_cleanup_disable();
    }

    fn dispatch_command(
        self: &Rc<Self>,
        document_generation: u64,
        stage: CommandStage,
        command: FixedSemanticCdpCommand,
    ) -> Result<(), ()> {
        {
            let mut state = self.shared.state.try_borrow_mut().map_err(|_| ())?;
            if state.in_flight.is_some() {
                return Err(());
            }
            if stage == CommandStage::RuntimeEnable {
                state.context_events_enabled = true;
                state.cleanup_disable_pending = false;
                state.cleanup_disable_attempts = 0;
            }
            state.in_flight = Some(InFlightCommand {
                document_generation,
                stage,
            });
        }
        let weak = Rc::downgrade(self);
        let callback_panicked = self.on_callback_panic.clone();
        let dispatched = call_cdp_async(
            &self.core,
            command,
            move |outcome| {
                if let Some(native) = weak.upgrade() {
                    native.on_command_completion(document_generation, stage, outcome);
                }
            },
            callback_panicked,
        );
        if dispatched.is_err() {
            if let Ok(mut state) = self.shared.state.try_borrow_mut() {
                if state.in_flight
                    == Some(InFlightCommand {
                        document_generation,
                        stage,
                    })
                {
                    state.in_flight = None;
                    if stage == CommandStage::RuntimeEnable {
                        state.context_events_enabled = false;
                    }
                }
            }
            return Err(());
        }
        Ok(())
    }

    fn on_command_completion(
        self: &Rc<Self>,
        document_generation: u64,
        stage: CommandStage,
        outcome: Result<String, NativeCdpFailure>,
    ) {
        let current = {
            let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                self.invariant_failed();
                return;
            };
            let expected = InFlightCommand {
                document_generation,
                stage,
            };
            if state.in_flight != Some(expected) {
                drop(state);
                self.invariant_failed();
                return;
            }
            state.in_flight = None;
            state.document_generation == document_generation
                && state.phase == DocumentPhase::Ready
                && state.pending.is_some()
        };

        if stage == CommandStage::CleanupRuntimeDisable {
            self.handle_disable_completion(outcome, true);
            return;
        }

        if !current {
            self.handle_stale_completion(stage, outcome);
            return;
        }
        if stage == CommandStage::RuntimeEnable && outcome.is_ok() {
            if let Ok(mut state) = self.shared.state.try_borrow_mut() {
                state.context_events_enabled = true;
                state.cleanup_disable_pending = false;
                state.cleanup_disable_attempts = 0;
            } else {
                self.fail_current(SemanticRuntimePortFailure::Transport, true);
                return;
            }
        }
        let response = match outcome {
            Ok(response) => response,
            Err(_) => {
                self.fail_current(SemanticRuntimePortFailure::Transport, true);
                return;
            }
        };
        match stage {
            CommandStage::GetFrameTree => {
                let root = match decode_root_frame(&response) {
                    Ok(root) => root,
                    Err(_) => {
                        self.fail_current(SemanticRuntimePortFailure::Transport, true);
                        return;
                    }
                };
                let discovery = match self.shared.state.try_borrow() {
                    Ok(state) => SemanticContextDiscovery::new(state.world.clone(), root),
                    Err(_) => {
                        self.fail_current(SemanticRuntimePortFailure::Transport, true);
                        return;
                    }
                };
                if let Ok(mut state) = self.shared.state.try_borrow_mut() {
                    state.discovery = Some(discovery);
                } else {
                    self.fail_current(SemanticRuntimePortFailure::Transport, true);
                    return;
                }
                let _ = self.dispatch_or_fail(
                    document_generation,
                    CommandStage::RuntimeEnable,
                    runtime_enable_command(),
                );
            }
            CommandStage::RuntimeEnable => {
                if decode_empty_success(&response).is_err() {
                    self.fail_current(SemanticRuntimePortFailure::Transport, true);
                    return;
                }
                let command = self
                    .shared
                    .state
                    .try_borrow()
                    .ok()
                    .and_then(|state| state.discovery.as_ref()?.create_world_command().ok());
                let Some(command) = command else {
                    self.fail_current(SemanticRuntimePortFailure::Transport, true);
                    return;
                };
                let _ = self.dispatch_or_fail(
                    document_generation,
                    CommandStage::CreateIsolatedWorld,
                    command,
                );
            }
            CommandStage::CreateIsolatedWorld => {
                let ready = {
                    let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                        self.fail_current(SemanticRuntimePortFailure::Transport, true);
                        return;
                    };
                    let Some(discovery) = state.discovery.as_mut() else {
                        drop(state);
                        self.fail_current(SemanticRuntimePortFailure::Transport, true);
                        return;
                    };
                    if discovery.record_created_context(&response).is_err() {
                        drop(state);
                        self.fail_current(SemanticRuntimePortFailure::Transport, true);
                        return;
                    }
                    discovery.resolved().ok().flatten().is_some()
                };
                if ready {
                    let _ = self.dispatch_or_fail(
                        document_generation,
                        CommandStage::RuntimeDisable,
                        runtime_disable_command(),
                    );
                }
            }
            CommandStage::RuntimeDisable => {
                if decode_empty_success(&response).is_err() {
                    self.fail_current(SemanticRuntimePortFailure::Transport, true);
                    return;
                }
                let context = {
                    let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                        self.fail_current(SemanticRuntimePortFailure::Transport, true);
                        return;
                    };
                    state.context_events_enabled = false;
                    state.cleanup_disable_pending = false;
                    state.cleanup_disable_attempts = 0;
                    let context = state
                        .discovery
                        .as_ref()
                        .and_then(|discovery| discovery.resolved().ok().flatten());
                    state.discovery = None;
                    state.installed_context = context.clone();
                    context
                };
                let Some(context) = context else {
                    self.fail_current(SemanticRuntimePortFailure::Transport, true);
                    return;
                };
                let command = install_runtime_in_context_command(&context).ok();
                let Some(command) = command else {
                    self.fail_current(SemanticRuntimePortFailure::Transport, true);
                    return;
                };
                let _ = self.dispatch_or_fail(
                    document_generation,
                    CommandStage::InstallRuntime,
                    command,
                );
            }
            CommandStage::InstallRuntime => {
                if decode_runtime_install_response(&response).is_err() {
                    self.fail_current(SemanticRuntimePortFailure::Transport, true);
                    return;
                }
                let command = self.shared.state.try_borrow().ok().and_then(|state| {
                    let context = state.installed_context.as_ref()?;
                    invoke_runtime_command(context, &state.pending.as_ref()?.invocation).ok()
                });
                let Some(command) = command else {
                    self.fail_current(SemanticRuntimePortFailure::Transport, true);
                    return;
                };
                let _ = self.dispatch_or_fail(document_generation, CommandStage::Invoke, command);
            }
            CommandStage::Invoke => self.settle_invocation(response),
            CommandStage::CleanupRuntimeDisable => {
                self.invariant_failed();
            }
        }
    }

    fn handle_stale_completion(
        self: &Rc<Self>,
        stage: CommandStage,
        outcome: Result<String, NativeCdpFailure>,
    ) {
        match stage {
            CommandStage::RuntimeEnable if outcome.is_ok() => {
                let valid = outcome
                    .as_deref()
                    .is_ok_and(|response| decode_empty_success(response).is_ok());
                if let Ok(mut state) = self.shared.state.try_borrow_mut() {
                    state.context_events_enabled = true;
                    state.cleanup_disable_pending = true;
                }
                if !valid {
                    self.invariant_failed();
                }
            }
            CommandStage::RuntimeDisable | CommandStage::CleanupRuntimeDisable => {
                self.handle_disable_completion(
                    outcome,
                    stage == CommandStage::CleanupRuntimeDisable,
                );
                return;
            }
            CommandStage::GetFrameTree
            | CommandStage::RuntimeEnable
            | CommandStage::CreateIsolatedWorld
            | CommandStage::InstallRuntime
            | CommandStage::Invoke => {}
        }
        self.maybe_dispatch_cleanup_disable();
    }

    fn handle_disable_completion(
        self: &Rc<Self>,
        outcome: Result<String, NativeCdpFailure>,
        cleanup_attempt: bool,
    ) {
        let valid = outcome
            .as_deref()
            .is_ok_and(|response| decode_empty_success(response).is_ok());
        if let Ok(mut state) = self.shared.state.try_borrow_mut() {
            state.discovery = None;
            if valid {
                state.context_events_enabled = false;
                state.cleanup_disable_pending = false;
                state.cleanup_disable_attempts = 0;
            } else {
                state.cleanup_disable_pending |= state.context_events_enabled;
                state.installed_context = None;
                if state.phase != DocumentPhase::Retired {
                    state.phase = DocumentPhase::Failed;
                }
            }
        } else {
            self.invariant_failed();
            return;
        }
        if !valid {
            self.invariant_failed();
            if !cleanup_attempt {
                self.maybe_dispatch_cleanup_disable();
            }
        }
    }

    fn dispatch_or_fail(
        self: &Rc<Self>,
        document_generation: u64,
        stage: CommandStage,
        command: FixedSemanticCdpCommand,
    ) -> Result<(), ()> {
        if self
            .dispatch_command(document_generation, stage, command)
            .is_err()
        {
            self.fail_current(SemanticRuntimePortFailure::Transport, true);
            Err(())
        } else {
            Ok(())
        }
    }

    fn on_context_created(self: &Rc<Self>, event_json: &str) {
        let (generation, ready) = {
            let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                self.invariant_failed();
                return;
            };
            if !state.context_events_enabled
                || state.phase != DocumentPhase::Ready
                || state.pending.is_none()
            {
                return;
            }
            let Some(discovery) = state.discovery.as_mut() else {
                drop(state);
                self.fail_current(SemanticRuntimePortFailure::Transport, true);
                return;
            };
            if discovery.observe_context_created(event_json).is_err() {
                drop(state);
                self.fail_current(SemanticRuntimePortFailure::Transport, true);
                return;
            }
            let ready = discovery.resolved().ok().flatten().is_some();
            (
                state.document_generation,
                ready && state.in_flight.is_none(),
            )
        };
        if ready {
            let _ = self.dispatch_or_fail(
                generation,
                CommandStage::RuntimeDisable,
                runtime_disable_command(),
            );
        }
    }

    fn settle_invocation(&self, response: String) {
        let (completion, outcome, invariant_failed) = {
            let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                self.invariant_failed();
                return;
            };
            let Some(pending) = state.pending.take() else {
                drop(state);
                self.invariant_failed();
                return;
            };
            let outcome = decode_invocation_response(&pending.invocation, &response);
            let (outcome, recoverable) = match outcome {
                Ok(snapshot) => (Ok(snapshot), true),
                Err(SemanticCdpInvocationError::Result(error)) => {
                    let recoverable = matches!(error, SemanticRuntimeResultError::Runtime(_));
                    (Err(SemanticRuntimePortFailure::Result(error)), recoverable)
                }
                Err(SemanticCdpInvocationError::Protocol(_)) => {
                    (Err(SemanticRuntimePortFailure::Transport), false)
                }
            };
            let completed = state.completed_invocations.checked_add(1);
            let count_valid =
                completed.filter(|count| *count <= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS);
            if let Some(completed) = count_valid {
                state.completed_invocations = completed;
            }
            if !recoverable || count_valid.is_none() {
                state.phase = DocumentPhase::Failed;
                state.installed_context = None;
            }
            (
                pending.completion,
                if count_valid.is_some() {
                    outcome
                } else {
                    Err(SemanticRuntimePortFailure::Transport)
                },
                !recoverable || count_valid.is_none(),
            )
        };
        self.invoke_completion(completion, outcome);
        if invariant_failed {
            self.invariant_failed();
        }
    }

    fn fail_current(self: &Rc<Self>, failure: SemanticRuntimePortFailure, invariant: bool) {
        let completion = {
            let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                self.invariant_failed();
                return;
            };
            state.phase = DocumentPhase::Failed;
            state.discovery = None;
            state.installed_context = None;
            state.cleanup_disable_pending |= state.context_events_enabled;
            state.pending.take().map(|pending| pending.completion)
        };
        if let Some(completion) = completion {
            self.invoke_completion(completion, Err(failure));
        }
        if invariant {
            self.invariant_failed();
        }
        self.maybe_dispatch_cleanup_disable();
    }

    fn maybe_dispatch_cleanup_disable(self: &Rc<Self>) {
        let generation = {
            let Ok(mut state) = self.shared.state.try_borrow_mut() else {
                self.invariant_failed();
                return;
            };
            if !state.cleanup_disable_pending
                || !state.context_events_enabled
                || state.in_flight.is_some()
                || state.phase == DocumentPhase::Retired
            {
                return;
            }
            if state.cleanup_disable_attempts >= MAX_CLEANUP_DISABLE_ATTEMPTS {
                drop(state);
                self.invariant_failed();
                return;
            }
            state.cleanup_disable_attempts += 1;
            state.document_generation
        };
        if self
            .dispatch_command(
                generation,
                CommandStage::CleanupRuntimeDisable,
                runtime_disable_command(),
            )
            .is_err()
        {
            if let Ok(mut state) = self.shared.state.try_borrow_mut() {
                state.phase = DocumentPhase::Failed;
            }
            self.invariant_failed();
        }
    }

    fn invoke_completion(
        &self,
        completion: SemanticCompletion,
        outcome: Result<SemanticSnapshot, SemanticRuntimePortFailure>,
    ) {
        if std::panic::catch_unwind(AssertUnwindSafe(|| completion(outcome))).is_err() {
            self.callback_panicked();
        }
    }

    fn invariant_failed(&self) {
        if std::panic::catch_unwind(AssertUnwindSafe(|| (self.on_invariant_failure)())).is_err() {
            self.callback_panicked();
        }
    }

    fn callback_panicked(&self) {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| (self.on_callback_panic)()));
    }
}

fn register_context_event(
    native: &Rc<NativeRuntime>,
) -> windows_core::Result<CdpEventRegistration> {
    let event_name = HSTRING::from("Runtime.executionContextCreated");
    // SAFETY: `native.core` is a live STA-bound COM interface and `event_name`
    // is a valid HSTRING for the duration of this synchronous getter. The
    // returned receiver owns its COM reference.
    let receiver = unsafe { native.core.GetDevToolsProtocolEventReceiver(&event_name)? };
    let weak = Rc::downgrade(native);
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
        let Some(native) = weak.upgrade() else {
            return Ok(());
        };
        let Some(args) = args else {
            native.fail_current(SemanticRuntimePortFailure::Transport, true);
            return Ok(());
        };
        let mut raw = PWSTR::null();
        // SAFETY: WebView2 supplied a live callback-argument COM interface and
        // `raw` is valid initialized out storage. The bounded helper consumes
        // and releases the allocated string exactly once on every result.
        if unsafe { args.ParameterObjectAsJson(&mut raw) }.is_err() {
            native.fail_current(SemanticRuntimePortFailure::Transport, true);
            return Ok(());
        }
        let Some(event) =
            super::take_pwstr_bounded(raw, MAX_CONTEXT_EVENT_BYTES, MAX_CONTEXT_EVENT_BYTES)
        else {
            native.fail_current(SemanticRuntimePortFailure::Transport, true);
            return Ok(());
        };
        native.on_context_created(&event);
        Ok(())
    }));
    let mut token = 0_i64;
    // SAFETY: receiver and handler are live reference-counted COM interfaces;
    // `token` is exact writable out storage. The registration retains the
    // handler until removal and returns its matching token synchronously.
    unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token)? };
    Ok(CdpEventRegistration {
        receiver,
        token,
        retired: false,
    })
}

struct CdpEventRegistration {
    receiver: ICoreWebView2DevToolsProtocolEventReceiver,
    token: i64,
    retired: bool,
}

impl CdpEventRegistration {
    fn retire(&mut self) -> bool {
        if self.retired {
            return false;
        }
        self.retired = true;
        // SAFETY: `receiver` remains live and `token` is the exact value
        // returned by its successful add call; retirement is single-shot.
        unsafe {
            self.receiver
                .remove_DevToolsProtocolEventReceived(self.token)
                .is_ok()
        }
    }
}

impl Drop for CdpEventRegistration {
    fn drop(&mut self) {
        if !self.retired {
            let _ = self.retire();
        }
    }
}

fn same_interface(left: &ICoreWebView2, right: &ICoreWebView2) -> bool {
    left.cast::<IUnknown>()
        .ok()
        .zip(right.cast::<IUnknown>().ok())
        .is_some_and(|(left, right)| left.as_raw() == right.as_raw())
}

fn call_cdp_async(
    core: &ICoreWebView2,
    command: FixedSemanticCdpCommand,
    completion: impl FnOnce(Result<String, NativeCdpFailure>) + 'static,
    on_callback_panic: Rc<dyn Fn()>,
) -> Result<(), NativeCdpFailure> {
    let response_limit = command.response_limit();
    let handler: ICoreWebView2CallDevToolsProtocolMethodCompletedHandler = BoundedCdpCompletion {
        response_limit,
        completion: RefCell::new(Some(Box::new(completion))),
        on_callback_panic,
    }
    .into();
    let method = HSTRING::from(command.method().as_str());
    let parameters = HSTRING::from(command.parameters());
    // SAFETY: `core`, both HSTRING arguments, and the COM handler are live for
    // dispatch. WebView2 retains the handler until its completion callback and
    // all of these Rc-backed values stay on the owning STA.
    unsafe { core.CallDevToolsProtocolMethod(&method, &parameters, &handler) }
        .map_err(|_| NativeCdpFailure::Native)
}

#[windows_core::implement(ICoreWebView2CallDevToolsProtocolMethodCompletedHandler)]
struct BoundedCdpCompletion {
    response_limit: usize,
    completion: RefCell<Option<NativeCompletion>>,
    on_callback_panic: Rc<dyn Fn()>,
}

impl ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl for BoundedCdpCompletion_Impl {
    fn Invoke(&self, result: HRESULT, response: &PCWSTR) -> windows_core::Result<()> {
        let outcome = if result.is_err() {
            Err(NativeCdpFailure::Native)
        } else {
            borrowed_pcwstr_bounded(response, self.response_limit, self.response_limit)
                .ok_or(NativeCdpFailure::ResponseLimit)
        };
        let completion = match self.completion.try_borrow_mut() {
            Ok(mut completion) => completion.take(),
            Err(_) => None,
        };
        let Some(completion) = completion else {
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| (self.on_callback_panic)()));
            return Ok(());
        };
        if std::panic::catch_unwind(AssertUnwindSafe(|| completion(outcome))).is_err() {
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| (self.on_callback_panic)()));
        }
        Ok(())
    }
}

fn borrowed_pcwstr_bounded(
    source: &PCWSTR,
    max_utf16_units: usize,
    max_utf8_bytes: usize,
) -> Option<String> {
    let pointer = source.as_ptr();
    if pointer.is_null() {
        return Some(String::new());
    }
    let mut length = 0_usize;
    while length <= max_utf16_units {
        // SAFETY: WebView2 owns a NUL-terminated result for the duration of
        // this callback. The scan stops at the policy ceiling plus one unit.
        if unsafe { pointer.add(length).read() } == 0 {
            // SAFETY: the bounded scan established this initialized prefix.
            let units = unsafe { std::slice::from_raw_parts(pointer, length) };
            let mut utf8_bytes = 0_usize;
            for character in char::decode_utf16(units.iter().copied()) {
                let character = character.unwrap_or(char::REPLACEMENT_CHARACTER);
                utf8_bytes = utf8_bytes.checked_add(character.len_utf8())?;
                if utf8_bytes > max_utf8_bytes {
                    return None;
                }
            }
            let mut value = String::new();
            value.try_reserve_exact(utf8_bytes).ok()?;
            for character in char::decode_utf16(units.iter().copied()) {
                value.push(character.unwrap_or(char::REPLACEMENT_CHARACTER));
            }
            return Some(value);
        }
        length += 1;
    }
    None
}
