//! Private production semantic runtime registration for owned agent contexts.
//!
//! WebKit's public evaluation APIs confer user activation, so this adapter
//! never evaluates JavaScript. The immutable document-start program instead
//! holds one Promise on a content-world-scoped reply handler. Native code may
//! answer that pull only with [`SemanticRuntimeInvocation`]'s closed grammar.

use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use block2::RcBlock;
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyObject, runtime::NSObject};
use objc2::{DefinedClass as _, MainThreadOnly, Message as _};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSString, NSUTF8StringEncoding};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandlerWithReply, WKUserContentController,
    WKUserScript, WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration,
};
use zephium_agentic::{
    SemanticRuntimeInvocation, SemanticRuntimeResultError, SemanticSnapshot,
    MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES, MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS,
    SEMANTIC_RUNTIME_CHANNEL_ACK, SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED,
    SEMANTIC_RUNTIME_CHANNEL_NAME, SEMANTIC_RUNTIME_CHANNEL_PULL,
    SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX, SEMANTIC_RUNTIME_CHANNEL_STOP,
    SEMANTIC_RUNTIME_PROGRAM,
};

const SEMANTIC_RUNTIME_WORLD_NAME_PREFIX: &str = "zephium-semantic-runtime-v1-";
const SEMANTIC_RUNTIME_FIXED_ERROR: &str = "zephium semantic channel refused";
static NEXT_SEMANTIC_RUNTIME_WORLD: AtomicU64 = AtomicU64::new(1);

type ReplyBlock = RcBlock<dyn Fn(*mut AnyObject, *mut NSString)>;
type SemanticCompletion = Box<dyn FnOnce(Result<SemanticSnapshot, AgentSemanticRuntimeFailure>)>;

/// Synchronous refusal before one exact invocation enters the native channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentSemanticRuntimeDispatchError {
    NotReady,
    Busy,
    Exhausted,
    Retired,
}

/// Closed terminal failure for one admitted semantic invocation.
#[derive(Debug)]
pub(crate) enum AgentSemanticRuntimeFailure {
    Dispatch(AgentSemanticRuntimeDispatchError),
    Cancelled,
    DocumentReplaced,
    RendererLost,
    TimedOut,
    Retired,
    Transport,
    Result(SemanticRuntimeResultError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentPhase {
    Loading,
    Ready,
    RendererLost,
    ExhaustionNoticePending,
    Exhausted,
    Failed,
    Retired,
}

struct PendingInvocation {
    invocation: SemanticRuntimeInvocation,
    completion: SemanticCompletion,
}

enum ReplyValue {
    Success(Box<str>),
    Error,
}

struct ReplyAction {
    reply: ReplyBlock,
    value: ReplyValue,
}

impl ReplyAction {
    fn success(reply: ReplyBlock, value: impl Into<Box<str>>) -> Self {
        Self {
            reply,
            value: ReplyValue::Success(value.into()),
        }
    }

    fn error(reply: ReplyBlock) -> Self {
        Self {
            reply,
            value: ReplyValue::Error,
        }
    }
}

struct CompletionAction {
    completion: SemanticCompletion,
    outcome: Result<SemanticSnapshot, AgentSemanticRuntimeFailure>,
}

#[derive(Default)]
struct ChannelActions {
    first_reply: Option<ReplyAction>,
    second_reply: Option<ReplyAction>,
    completion: Option<CompletionAction>,
    invariant_failed: bool,
}

impl ChannelActions {
    fn push_reply(&mut self, reply: ReplyAction) {
        if self.first_reply.is_none() {
            self.first_reply = Some(reply);
        } else if self.second_reply.is_none() {
            self.second_reply = Some(reply);
        } else {
            self.invariant_failed = true;
        }
    }
}

struct SemanticRuntimeChannelState {
    expected_view: Option<usize>,
    active_world: Option<usize>,
    phase: DocumentPhase,
    pull: Option<ReplyBlock>,
    pending: Option<PendingInvocation>,
    awaiting_result: bool,
    completed_invocations: u16,
}

impl Default for SemanticRuntimeChannelState {
    fn default() -> Self {
        Self {
            expected_view: None,
            active_world: None,
            phase: DocumentPhase::Loading,
            pull: None,
            pending: None,
            awaiting_result: false,
            completed_invocations: 0,
        }
    }
}

impl SemanticRuntimeChannelState {
    fn bind_world(&mut self, world: &WKContentWorld) -> Result<(), ()> {
        if self.phase != DocumentPhase::Loading || self.active_world.is_some() {
            return Err(());
        }
        self.active_world = Some(std::ptr::from_ref(world).addr());
        Ok(())
    }

    fn world_matches(&self, world: &WKContentWorld) -> bool {
        self.active_world == Some(std::ptr::from_ref(world).addr())
    }

    fn bind_view(&mut self, view: &WKWebView) -> Result<(), ()> {
        let pointer = std::ptr::from_ref(view).addr();
        match self.expected_view {
            None if self.phase == DocumentPhase::Loading => {
                self.expected_view = Some(pointer);
                Ok(())
            }
            Some(expected) if expected == pointer => Ok(()),
            None | Some(_) => Err(()),
        }
    }

    fn dispatch(
        &mut self,
        invocation: SemanticRuntimeInvocation,
        completion: SemanticCompletion,
    ) -> Result<ChannelActions, (AgentSemanticRuntimeDispatchError, SemanticCompletion)> {
        if self.expected_view.is_none() || self.phase == DocumentPhase::Loading {
            return Err((AgentSemanticRuntimeDispatchError::NotReady, completion));
        }
        match self.phase {
            DocumentPhase::Ready => {}
            DocumentPhase::ExhaustionNoticePending | DocumentPhase::Exhausted => {
                return Err((AgentSemanticRuntimeDispatchError::Exhausted, completion));
            }
            DocumentPhase::RendererLost | DocumentPhase::Failed | DocumentPhase::Retired => {
                return Err((AgentSemanticRuntimeDispatchError::Retired, completion));
            }
            DocumentPhase::Loading => unreachable!(),
        }
        if self.pending.is_some() || self.awaiting_result {
            return Err((AgentSemanticRuntimeDispatchError::Busy, completion));
        }
        if self.completed_invocations >= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS {
            self.phase = DocumentPhase::Exhausted;
            return Err((AgentSemanticRuntimeDispatchError::Exhausted, completion));
        }
        self.pending = Some(PendingInvocation {
            invocation,
            completion,
        });
        Ok(self.prepare_pump())
    }

    fn document_committed(&mut self) -> ChannelActions {
        let mut actions = ChannelActions::default();
        if self.phase == DocumentPhase::Loading {
            self.phase = DocumentPhase::Ready;
        } else {
            actions = self.invalidate_current(AgentSemanticRuntimeFailure::DocumentReplaced);
            actions.invariant_failed = true;
            if self.phase != DocumentPhase::Retired {
                self.phase = DocumentPhase::Failed;
            }
        }
        // Wry may report the construction-only about:blank commit while the
        // builder still owns the new WKWebView and before `bind_view` can run.
        // Dispatch remains closed until the exact returned view is bound.
        actions.invariant_failed |= self.active_world.is_none();
        actions
    }

    fn begin_document_load(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(AgentSemanticRuntimeFailure::DocumentReplaced);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Loading;
            self.completed_invocations = 0;
            self.active_world = None;
        }
        actions
    }

    fn registration_failed(&mut self) -> ChannelActions {
        let mut actions = self.invalidate_current(AgentSemanticRuntimeFailure::Transport);
        self.active_world = None;
        self.phase = DocumentPhase::Failed;
        actions.invariant_failed = true;
        actions
    }

    fn renderer_lost(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(AgentSemanticRuntimeFailure::RendererLost);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::RendererLost;
        }
        actions
    }

    fn cancel(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(AgentSemanticRuntimeFailure::Cancelled);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Failed;
        }
        actions
    }

    fn timeout(
        &mut self,
        invocation: zephium_agentic::SemanticInvocationId,
    ) -> (ChannelActions, bool) {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.invocation.invocation() != invocation)
        {
            return (ChannelActions::default(), false);
        }
        let actions = self.invalidate_current(AgentSemanticRuntimeFailure::TimedOut);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Failed;
        }
        (actions, true)
    }

    fn retire(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(AgentSemanticRuntimeFailure::Retired);
        self.active_world = None;
        self.phase = DocumentPhase::Retired;
        actions
    }

    fn fail_transport(&mut self, current: Option<ReplyBlock>) -> ChannelActions {
        let mut actions = self.invalidate_current(AgentSemanticRuntimeFailure::Transport);
        if let Some(reply) = current {
            actions.push_reply(ReplyAction::error(reply));
        }
        self.phase = DocumentPhase::Failed;
        actions.invariant_failed = true;
        actions
    }

    fn invalidate_current(&mut self, failure: AgentSemanticRuntimeFailure) -> ChannelActions {
        let mut actions = ChannelActions::default();
        if let Some(pull) = self.pull.take() {
            actions.push_reply(ReplyAction::success(pull, SEMANTIC_RUNTIME_CHANNEL_STOP));
        }
        self.awaiting_result = false;
        if let Some(pending) = self.pending.take() {
            actions.completion = Some(CompletionAction {
                completion: pending.completion,
                outcome: Err(failure),
            });
        }
        actions
    }

    fn on_message(&mut self, body: &str, reply: ReplyBlock) -> ChannelActions {
        if body == SEMANTIC_RUNTIME_CHANNEL_PULL {
            return self.on_pull(reply);
        }
        if body == SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED {
            return self.on_exhausted(reply);
        }
        if let Some(result) = body.strip_prefix(SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX) {
            return self.on_result(result.as_bytes(), reply);
        }
        self.fail_transport(Some(reply))
    }

    fn on_pull(&mut self, reply: ReplyBlock) -> ChannelActions {
        if matches!(
            self.phase,
            DocumentPhase::RendererLost
                | DocumentPhase::ExhaustionNoticePending
                | DocumentPhase::Exhausted
                | DocumentPhase::Failed
                | DocumentPhase::Retired
        ) {
            let mut actions = ChannelActions::default();
            actions.push_reply(ReplyAction::success(reply, SEMANTIC_RUNTIME_CHANNEL_STOP));
            return actions;
        }
        if self.pull.is_some() || self.awaiting_result {
            return self.fail_transport(Some(reply));
        }
        self.pull = Some(reply);
        self.prepare_pump()
    }

    fn on_result(&mut self, bytes: &[u8], reply: ReplyBlock) -> ChannelActions {
        if !self.awaiting_result {
            return self.fail_transport(Some(reply));
        }
        let Some(pending) = self.pending.take() else {
            return self.fail_transport(Some(reply));
        };
        self.awaiting_result = false;
        let Some(completed) = self.completed_invocations.checked_add(1) else {
            let mut actions = self.fail_transport(Some(reply));
            actions.completion = Some(CompletionAction {
                completion: pending.completion,
                outcome: Err(AgentSemanticRuntimeFailure::Transport),
            });
            return actions;
        };
        self.completed_invocations = completed;
        if completed == MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS {
            self.phase = DocumentPhase::ExhaustionNoticePending;
        }

        let outcome = pending
            .invocation
            .decode_result(bytes)
            .map_err(AgentSemanticRuntimeFailure::Result);
        let recoverable = matches!(
            &outcome,
            Ok(_)
                | Err(AgentSemanticRuntimeFailure::Result(
                    SemanticRuntimeResultError::Runtime(_)
                ))
        );
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(
            reply,
            if recoverable {
                SEMANTIC_RUNTIME_CHANNEL_ACK
            } else {
                SEMANTIC_RUNTIME_CHANNEL_STOP
            },
        ));
        actions.completion = Some(CompletionAction {
            completion: pending.completion,
            outcome,
        });
        if !recoverable {
            self.phase = DocumentPhase::Failed;
            actions.invariant_failed = true;
        }
        actions
    }

    fn on_exhausted(&mut self, reply: ReplyBlock) -> ChannelActions {
        if self.completed_invocations != MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS
            || self.pending.is_some()
            || self.awaiting_result
            || self.pull.is_some()
            || self.phase != DocumentPhase::ExhaustionNoticePending
        {
            return self.fail_transport(Some(reply));
        }
        self.phase = DocumentPhase::Exhausted;
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(reply, SEMANTIC_RUNTIME_CHANNEL_ACK));
        actions
    }

    fn prepare_pump(&mut self) -> ChannelActions {
        let mut actions = ChannelActions::default();
        if self.phase != DocumentPhase::Ready
            || self.awaiting_result
            || self.pending.is_none()
            || self.pull.is_none()
        {
            return actions;
        }
        let request = self
            .pending
            .as_ref()
            .map(|pending| pending.invocation.as_str().to_owned().into_boxed_str());
        let Some(request) = request else {
            actions.invariant_failed = true;
            return actions;
        };
        let Some(pull) = self.pull.take() else {
            actions.invariant_failed = true;
            return actions;
        };
        self.awaiting_result = true;
        actions.push_reply(ReplyAction::success(pull, request));
        actions
    }
}

#[derive(Clone)]
pub(crate) struct AgentSemanticRuntimeController {
    state: Rc<RefCell<SemanticRuntimeChannelState>>,
    on_invariant_failure: Rc<dyn Fn()>,
    on_callback_panic: Rc<dyn Fn()>,
}

impl AgentSemanticRuntimeController {
    fn new(on_invariant_failure: Rc<dyn Fn()>, on_callback_panic: Rc<dyn Fn()>) -> Self {
        Self {
            state: Rc::new(RefCell::new(SemanticRuntimeChannelState::default())),
            on_invariant_failure,
            on_callback_panic,
        }
    }

    pub(crate) fn bind_view(&self, view: &WKWebView) -> Result<(), ()> {
        self.state.try_borrow_mut().map_err(|_| ())?.bind_view(view)
    }

    fn bind_world(&self, world: &WKContentWorld) -> Result<(), ()> {
        self.state
            .try_borrow_mut()
            .map_err(|_| ())?
            .bind_world(world)
    }

    fn world_matches(&self, world: &WKContentWorld) -> Result<bool, ()> {
        Ok(self
            .state
            .try_borrow()
            .map_err(|_| ())?
            .world_matches(world))
    }

    pub(crate) fn dispatch(
        &self,
        invocation: SemanticRuntimeInvocation,
        completion: impl FnOnce(Result<SemanticSnapshot, AgentSemanticRuntimeFailure>) + 'static,
    ) -> Result<(), AgentSemanticRuntimeDispatchError> {
        let completion: SemanticCompletion = Box::new(completion);
        let dispatched = match self.state.try_borrow_mut() {
            Ok(mut state) => state.dispatch(invocation, completion),
            Err(_) => Err((AgentSemanticRuntimeDispatchError::Busy, completion)),
        };
        match dispatched {
            Ok(actions) => {
                self.execute(actions);
                Ok(())
            }
            Err((failure, completion)) => {
                invoke_completion(
                    CompletionAction {
                        completion,
                        outcome: Err(AgentSemanticRuntimeFailure::Dispatch(failure)),
                    },
                    self.on_callback_panic.as_ref(),
                );
                Err(failure)
            }
        }
    }

    pub(crate) fn begin_document_load(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::begin_document_load);
        self.execute(actions);
    }

    fn registration_failed(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::registration_failed);
        self.execute(actions);
    }

    pub(crate) fn document_committed(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::document_committed);
        self.execute(actions);
    }

    pub(crate) fn renderer_lost(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::renderer_lost);
        self.execute(actions);
    }

    pub(crate) fn cancel(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::cancel);
        self.execute(actions);
    }

    pub(crate) fn timeout(&self, invocation: zephium_agentic::SemanticInvocationId) -> bool {
        let (actions, matched) = match self.state.try_borrow_mut() {
            Ok(mut state) => state.timeout(invocation),
            Err(_) => (
                ChannelActions {
                    invariant_failed: true,
                    ..ChannelActions::default()
                },
                false,
            ),
        };
        self.execute(actions);
        matched
    }

    pub(crate) fn pending_for_audit(&self) -> Option<bool> {
        let state = self.state.try_borrow().ok()?;
        let pending = state.pending.is_some();
        let world_valid = if state.phase == DocumentPhase::Retired {
            state.active_world.is_none()
        } else {
            state.active_world.is_some()
        };
        let valid = world_valid
            && state.completed_invocations <= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS
            && (!state.awaiting_result || (pending && state.pull.is_none()))
            && (state.pull.is_none() || (!state.awaiting_result && !pending))
            && (!matches!(
                state.phase,
                DocumentPhase::RendererLost
                    | DocumentPhase::ExhaustionNoticePending
                    | DocumentPhase::Exhausted
                    | DocumentPhase::Failed
                    | DocumentPhase::Retired
            ) || (!pending && !state.awaiting_result && state.pull.is_none()));
        valid.then_some(pending)
    }

    fn retire(&self) -> bool {
        let (actions, clean) = match self.state.try_borrow_mut() {
            Ok(mut state) => (state.retire(), true),
            Err(_) => {
                let actions = ChannelActions {
                    invariant_failed: true,
                    ..ChannelActions::default()
                };
                (actions, false)
            }
        };
        self.execute(actions);
        clean
    }

    fn transition(
        &self,
        transition: fn(&mut SemanticRuntimeChannelState) -> ChannelActions,
    ) -> ChannelActions {
        self.state.try_borrow_mut().map_or_else(
            |_| ChannelActions {
                invariant_failed: true,
                ..ChannelActions::default()
            },
            |mut state| transition(&mut state),
        )
    }

    fn expected_view(&self) -> Option<usize> {
        self.state.try_borrow().ok()?.expected_view
    }

    fn receive(&self, body: &str, reply: ReplyBlock) {
        let actions = match self.state.try_borrow_mut() {
            Ok(mut state) => state.on_message(body, reply),
            Err(_) => {
                let mut actions = ChannelActions::default();
                actions.push_reply(ReplyAction::error(reply));
                actions.invariant_failed = true;
                actions
            }
        };
        self.execute(actions);
    }

    fn reject(&self, reply: ReplyBlock, invariant: bool) {
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(reply, SEMANTIC_RUNTIME_CHANNEL_STOP));
        actions.invariant_failed = invariant;
        self.execute(actions);
    }

    fn execute(&self, mut actions: ChannelActions) {
        let mut reply_failed = false;
        for reply in [actions.first_reply.take(), actions.second_reply.take()]
            .into_iter()
            .flatten()
        {
            reply_failed |= !send_reply(reply);
        }
        if actions.invariant_failed || reply_failed {
            invoke_unit_callback(
                self.on_invariant_failure.as_ref(),
                self.on_callback_panic.as_ref(),
            );
        }
        if reply_failed {
            let mut failure = self.state.try_borrow_mut().map_or_else(
                |_| ChannelActions::default(),
                |mut state| state.fail_transport(None),
            );
            for reply in [failure.first_reply.take(), failure.second_reply.take()]
                .into_iter()
                .flatten()
            {
                let _ = send_reply(reply);
            }
            if let Some(completion) = failure.completion.take() {
                invoke_completion(completion, self.on_callback_panic.as_ref());
            }
        }
        if let Some(completion) = actions.completion.take() {
            invoke_completion(completion, self.on_callback_panic.as_ref());
        }
    }
}

fn invoke_completion(completion: CompletionAction, on_panic: &dyn Fn()) {
    if std::panic::catch_unwind(AssertUnwindSafe(|| {
        (completion.completion)(completion.outcome);
    }))
    .is_err()
    {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(on_panic));
    }
}

fn invoke_unit_callback(callback: &dyn Fn(), on_panic: &dyn Fn()) {
    if std::panic::catch_unwind(AssertUnwindSafe(callback)).is_err() {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(on_panic));
    }
}

fn send_reply(action: ReplyAction) -> bool {
    objc2::exception::catch(AssertUnwindSafe(|| match action.value {
        ReplyValue::Success(value) => {
            let value = NSString::from_str(&value);
            let pointer = Retained::as_ptr(&value).cast_mut().cast::<AnyObject>();
            action.reply.call((pointer, null_mut()));
        }
        ReplyValue::Error => {
            let error = NSString::from_str(SEMANTIC_RUNTIME_FIXED_ERROR);
            action
                .reply
                .call((null_mut(), Retained::as_ptr(&error).cast_mut()));
        }
    }))
    .is_ok()
}

struct SemanticMessageHandlerIvars {
    controller: Retained<WKUserContentController>,
    world: Retained<WKContentWorld>,
    handler_name: Retained<NSString>,
    channel: AgentSemanticRuntimeController,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumSemanticRuntimeMessageHandler"]
    #[ivars = SemanticMessageHandlerIvars]
    struct SemanticMessageHandler;

    unsafe impl NSObjectProtocol for SemanticMessageHandler {}

    unsafe impl WKScriptMessageHandlerWithReply for SemanticMessageHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:replyHandler:))]
        unsafe fn user_content_controller_did_receive_script_message_reply_handler(
            &self,
            controller: &WKUserContentController,
            message: &WKScriptMessage,
            reply: &block2::DynBlock<dyn Fn(*mut AnyObject, *mut NSString)>,
        ) {
            let ivars = self.ivars();
            let reply = reply.copy();
            let expected_view = ivars.channel.expected_view();
            let (world, name, webview, frame) = unsafe {
                (
                    message.world(),
                    message.name(),
                    message.webView(),
                    message.frameInfo(),
                )
            };
            if !std::ptr::eq(controller, &*ivars.controller)
                || Retained::as_ptr(&world) != Retained::as_ptr(&ivars.world)
                || !name.isEqualToString(&ivars.handler_name)
            {
                ivars.channel.reject(reply, true);
                return;
            }
            match ivars.channel.world_matches(&world) {
                Ok(true) => {}
                // Removing a document's world cannot retract a callback that
                // WebKit already delivered. That old epoch has no authority.
                Ok(false) => {
                    ivars.channel.reject(reply, false);
                    return;
                }
                Err(()) => {
                    ivars.channel.reject(reply, true);
                    return;
                }
            }
            let Some(expected_view) = expected_view else {
                // The construction-only about:blank can execute before the
                // returned WKWebView is bound. It receives no authority and
                // exits; later documents install a fresh runtime.
                ivars.channel.reject(reply, false);
                return;
            };
            let view_matches = webview
                .as_ref()
                .is_some_and(|view| std::ptr::from_ref(&**view).addr() == expected_view);
            let frame_view_matches = unsafe { frame.webView() }
                .as_ref()
                .is_some_and(|view| std::ptr::from_ref(&**view).addr() == expected_view);
            if !view_matches || !frame_view_matches || !unsafe { frame.isMainFrame() } {
                ivars.channel.reject(reply, true);
                return;
            }
            let body = unsafe { message.body() };
            let Ok(body) = body.downcast::<NSString>() else {
                ivars.channel.reject(reply, true);
                return;
            };
            if body.length() > MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES
                || body.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                    > MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES
            {
                ivars.channel.reject(reply, true);
                return;
            }
            ivars.channel.receive(&body.to_string(), reply);
        }
    }
);

struct SemanticRuntimeEpochRegistration {
    world: Retained<WKContentWorld>,
    script: Retained<WKUserScript>,
    handler: Retained<SemanticMessageHandler>,
}

fn next_semantic_runtime_world(mtm: MainThreadMarker) -> Result<Retained<WKContentWorld>, ()> {
    let identifier = NEXT_SEMANTIC_RUNTIME_WORLD
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| ())?;
    let name = NSString::from_str(&format!(
        "{SEMANTIC_RUNTIME_WORLD_NAME_PREFIX}{identifier:016x}"
    ));
    let world = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        WKContentWorld::worldWithName(&name, mtm)
    }))
    .map_err(|_| ())?;
    if unsafe { world.name() }
        .as_ref()
        .is_none_or(|actual| !actual.isEqualToString(&name))
    {
        return Err(());
    }
    Ok(world)
}

fn clear_semantic_runtime_controller(
    controller: &WKUserContentController,
    handler_name: &NSString,
    world: Option<&WKContentWorld>,
) -> bool {
    let removed = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        if let Some(world) = world {
            controller.removeScriptMessageHandlerForName_contentWorld(handler_name, world);
        }
        // This controller is created solely for the owned agent view. The
        // sweep makes a partial registration failure mechanically empty.
        controller.removeAllScriptMessageHandlers();
        controller.removeAllUserScripts();
    }))
    .is_ok();
    removed && unsafe { controller.userScripts() }.count() == 0
}

fn install_semantic_runtime_epoch(
    controller: &WKUserContentController,
    handler_name: &NSString,
    channel: &AgentSemanticRuntimeController,
    mtm: MainThreadMarker,
) -> Result<SemanticRuntimeEpochRegistration, ()> {
    let world = next_semantic_runtime_world(mtm)?;
    let handler = SemanticMessageHandler::alloc(mtm).set_ivars(SemanticMessageHandlerIvars {
        controller: controller.retain(),
        world: world.clone(),
        handler_name: handler_name.retain(),
        channel: channel.clone(),
    });
    let handler: Retained<SemanticMessageHandler> = unsafe { msg_send![super(handler), init] };
    let protocol_handler = objc2::runtime::ProtocolObject::from_ref(&*handler);
    let added_handler = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        controller.addScriptMessageHandlerWithReply_contentWorld_name(
            protocol_handler,
            &world,
            handler_name,
        );
    }))
    .is_ok();
    if !added_handler {
        let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
        return Err(());
    }

    let source = NSString::from_str(SEMANTIC_RUNTIME_PROGRAM.source());
    let script = match objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &source,
            WKUserScriptInjectionTime::AtDocumentStart,
            true,
            &world,
        )
    })) {
        Ok(script) => script,
        Err(_) => {
            let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
            return Err(());
        }
    };
    let added = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        controller.addUserScript(&script);
    }))
    .is_ok();
    if !added || channel.bind_world(&world).is_err() {
        let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
        return Err(());
    }
    Ok(SemanticRuntimeEpochRegistration {
        world,
        script,
        handler,
    })
}

/// Retains one production semantic world, handler, and document-start script.
///
/// The registration rotates to a fresh world before every authorized native
/// load. WebKit retains the posting JavaScript context until an asynchronous
/// reply is delivered, so a fixed world cannot distinguish a late callback
/// from the replaced document through public `WKFrameInfo` alone.
pub(crate) struct AgentSemanticRuntimeRegistration {
    controller: Retained<WKUserContentController>,
    handler_name: Retained<NSString>,
    active: Option<SemanticRuntimeEpochRegistration>,
    channel: AgentSemanticRuntimeController,
    retired: bool,
}

impl AgentSemanticRuntimeRegistration {
    pub(crate) fn install(
        configuration: &WKWebViewConfiguration,
        on_invariant_failure: Rc<dyn Fn()>,
        on_callback_panic: Rc<dyn Fn()>,
    ) -> Result<Self, ()> {
        let mtm = MainThreadMarker::new().ok_or(())?;
        let controller = unsafe { configuration.userContentController() };
        if unsafe { controller.userScripts() }.count() != 0 {
            return Err(());
        }
        let handler_name = NSString::from_str(SEMANTIC_RUNTIME_CHANNEL_NAME);
        let channel = AgentSemanticRuntimeController::new(on_invariant_failure, on_callback_panic);
        let active = install_semantic_runtime_epoch(&controller, &handler_name, &channel, mtm)?;

        let registration = Self {
            controller,
            handler_name,
            active: Some(active),
            channel,
            retired: false,
        };
        registration.attest_configuration(configuration)?;
        Ok(registration)
    }

    pub(crate) fn bind_view(&self, view: &WKWebView) -> Result<(), ()> {
        self.channel.bind_view(view)
    }

    pub(crate) const fn controller(&self) -> &AgentSemanticRuntimeController {
        &self.channel
    }

    /// Revokes the old document world and installs the immutable program in a
    /// fresh one before native navigation can begin.
    pub(crate) fn prepare_document_load(&mut self) -> Result<(), ()> {
        if self.retired {
            return Err(());
        }
        self.channel.begin_document_load();
        let Some(old) = self.active.take() else {
            self.channel.registration_failed();
            return Err(());
        };
        if !clear_semantic_runtime_controller(
            &self.controller,
            &self.handler_name,
            Some(&old.world),
        ) {
            self.channel.registration_failed();
            return Err(());
        }
        drop(old);
        let Some(mtm) = MainThreadMarker::new() else {
            self.channel.registration_failed();
            return Err(());
        };
        match install_semantic_runtime_epoch(
            &self.controller,
            &self.handler_name,
            &self.channel,
            mtm,
        ) {
            Ok(active) => {
                self.active = Some(active);
                if self.attest_controller().is_ok() {
                    Ok(())
                } else {
                    let world = self.active.as_ref().map(|active| &*active.world);
                    let _ = clear_semantic_runtime_controller(
                        &self.controller,
                        &self.handler_name,
                        world,
                    );
                    self.active = None;
                    self.channel.registration_failed();
                    Err(())
                }
            }
            Err(()) => {
                self.channel.registration_failed();
                Err(())
            }
        }
    }

    pub(crate) fn attest_configuration(
        &self,
        configuration: &WKWebViewConfiguration,
    ) -> Result<(), ()> {
        if self.retired {
            return Err(());
        }
        let actual_controller = unsafe { configuration.userContentController() };
        if Retained::as_ptr(&actual_controller) != Retained::as_ptr(&self.controller) {
            return Err(());
        }
        self.attest_controller()
    }

    fn attest_controller(&self) -> Result<(), ()> {
        let active = self.active.as_ref().ok_or(())?;
        if self.channel.world_matches(&active.world) != Ok(true) {
            return Err(());
        }
        let scripts = unsafe { self.controller.userScripts() };
        if scripts.count() != 1 {
            return Err(());
        }
        let script = scripts.objectAtIndex(0);
        if Retained::as_ptr(&script) != Retained::as_ptr(&active.script)
            || unsafe { script.source() }.to_string() != SEMANTIC_RUNTIME_PROGRAM.source()
            || unsafe { script.injectionTime() } != WKUserScriptInjectionTime::AtDocumentStart
            || !unsafe { script.isForMainFrameOnly() }
        {
            return Err(());
        }
        let _ = &active.handler;
        Ok(())
    }

    pub(crate) fn retire(mut self) -> Result<(), ()> {
        let channel_clean = self.channel.retire();
        let world = self.active.as_ref().map(|active| &*active.world);
        let removed =
            clear_semantic_runtime_controller(&self.controller, &self.handler_name, world);
        self.active = None;
        self.retired = true;
        if channel_clean && removed {
            Ok(())
        } else {
            Err(())
        }
    }
}

impl Drop for AgentSemanticRuntimeRegistration {
    fn drop(&mut self) {
        if self.retired {
            return;
        }
        let _ = self.channel.retire();
        let world = self.active.as_ref().map(|active| &*active.world);
        let _ = clear_semantic_runtime_controller(&self.controller, &self.handler_name, world);
        self.active = None;
        self.retired = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_agentic::{
        encode_semantic_runtime_invocation, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameId, SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticRuntimeBudget, SemanticRuntimeFault, SemanticSnapshotGeneration,
    };
    use zephium_core::ids::ProfileId;

    fn reply() -> ReplyBlock {
        RcBlock::new(|_: *mut AnyObject, _: *mut NSString| {})
    }

    fn invocation(
        invocation: u64,
        generation: SemanticSnapshotGeneration,
    ) -> SemanticRuntimeInvocation {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::from(91),
            ContextKind::Owned,
        );
        let capabilities =
            ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settlement");
        let context = registry.join(identity.id()).expect("join");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://semantic.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        encode_semantic_runtime_invocation(
            &request,
            frame,
            SemanticInvocationId::new(invocation).expect("invocation"),
            generation,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .expect("encode")
    }

    fn success_value(action: ReplyAction) -> Box<str> {
        match action.value {
            ReplyValue::Success(value) => value,
            ReplyValue::Error => panic!("unexpected reply error"),
        }
    }

    #[test]
    fn loading_holds_one_pull_until_exact_commit_and_decodes_one_result() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(7),
            active_world: Some(8),
            ..SemanticRuntimeChannelState::default()
        };
        let actions = state.on_message(SEMANTIC_RUNTIME_CHANNEL_PULL, reply());
        assert!(actions.first_reply.is_none());
        assert!(state.pull.is_some());

        let actions = state.document_committed();
        assert!(!actions.invariant_failed);
        assert!(actions.first_reply.is_none());
        assert_eq!(state.phase, DocumentPhase::Ready);

        let invocation = invocation(11, SemanticSnapshotGeneration::INITIAL);
        let encoded = invocation.as_str().to_owned();
        let actions = state
            .dispatch(invocation, Box::new(|_| {}))
            .unwrap_or_else(|_| panic!("dispatch"));
        assert_eq!(
            success_value(actions.first_reply.expect("request reply")).as_ref(),
            encoded
        );
        assert!(state.awaiting_result);
        assert!(state.pending.is_some());

        let actions = state.on_message("R1:E1:busy", reply());
        assert_eq!(
            success_value(actions.first_reply.expect("acknowledgement")).as_ref(),
            SEMANTIC_RUNTIME_CHANNEL_ACK
        );
        assert!(matches!(
            actions.completion.expect("completion").outcome,
            Err(AgentSemanticRuntimeFailure::Result(
                SemanticRuntimeResultError::Runtime(SemanticRuntimeFault::Busy)
            ))
        ));
        assert_eq!(state.completed_invocations, 1);
        assert!(!state.awaiting_result);
        assert!(state.pending.is_none());
    }

    #[test]
    fn construction_commit_before_view_binding_never_opens_native_dispatch() {
        let mut state = SemanticRuntimeChannelState {
            active_world: Some(8),
            ..SemanticRuntimeChannelState::default()
        };
        let actions = state.document_committed();
        assert!(!actions.invariant_failed);
        assert_eq!(state.phase, DocumentPhase::Ready);
        assert!(matches!(
            state.dispatch(
                invocation(10, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            ),
            Err((AgentSemanticRuntimeDispatchError::NotReady, _))
        ));
    }

    #[test]
    fn document_load_renderer_loss_and_retirement_settle_every_retained_owner() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(9),
            active_world: Some(10),
            phase: DocumentPhase::Ready,
            ..SemanticRuntimeChannelState::default()
        };
        let invocation = invocation(12, SemanticSnapshotGeneration::INITIAL);
        assert!(state
            .dispatch(invocation, Box::new(|_| {}))
            .unwrap_or_else(|_| panic!("dispatch"))
            .first_reply
            .is_none());
        let actions = state.begin_document_load();
        assert!(matches!(
            actions.completion.expect("replacement").outcome,
            Err(AgentSemanticRuntimeFailure::DocumentReplaced)
        ));
        assert_eq!(state.phase, DocumentPhase::Loading);
        assert_eq!(state.active_world, None);
        state.active_world = Some(11);

        assert!(state
            .on_message(SEMANTIC_RUNTIME_CHANNEL_PULL, reply())
            .first_reply
            .is_none());
        assert!(!state.document_committed().invariant_failed);
        let actions = state.renderer_lost();
        assert_eq!(
            success_value(actions.first_reply.expect("stop")).as_ref(),
            SEMANTIC_RUNTIME_CHANNEL_STOP
        );
        assert_eq!(state.phase, DocumentPhase::RendererLost);
        assert!(state.pending.is_none());
        assert!(state.pull.is_none());

        let actions = state.retire();
        assert!(actions.first_reply.is_none());
        assert_eq!(state.phase, DocumentPhase::Retired);
        assert_eq!(state.active_world, None);
    }

    #[test]
    fn document_invocation_budget_and_audit_invariants_fail_closed() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(13),
            active_world: Some(14),
            phase: DocumentPhase::ExhaustionNoticePending,
            completed_invocations: MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS,
            ..SemanticRuntimeChannelState::default()
        };
        assert!(matches!(
            state.dispatch(
                invocation(13, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            ),
            Err((AgentSemanticRuntimeDispatchError::Exhausted, _))
        ));
        assert_eq!(state.phase, DocumentPhase::ExhaustionNoticePending);
        let actions = state.on_message(SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED, reply());
        assert_eq!(
            success_value(actions.first_reply.expect("exhaustion ack")).as_ref(),
            SEMANTIC_RUNTIME_CHANNEL_ACK
        );
        assert_eq!(state.phase, DocumentPhase::Exhausted);

        let mut invalid = SemanticRuntimeChannelState {
            expected_view: Some(17),
            active_world: Some(18),
            phase: DocumentPhase::Ready,
            awaiting_result: true,
            ..SemanticRuntimeChannelState::default()
        };
        let controller = AgentSemanticRuntimeController {
            state: Rc::new(RefCell::new(invalid)),
            on_invariant_failure: Rc::new(|| {}),
            on_callback_panic: Rc::new(|| {}),
        };
        assert_eq!(controller.pending_for_audit(), None);

        invalid = SemanticRuntimeChannelState {
            expected_view: Some(19),
            active_world: Some(20),
            phase: DocumentPhase::Ready,
            ..SemanticRuntimeChannelState::default()
        };
        *controller.state.borrow_mut() = invalid;
        assert_eq!(controller.pending_for_audit(), Some(false));

        controller.state.borrow_mut().active_world = None;
        assert_eq!(controller.pending_for_audit(), None);
    }

    #[test]
    fn timeout_is_invocation_exact_and_permanently_stops_the_document_channel() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(23),
            active_world: Some(24),
            phase: DocumentPhase::Ready,
            ..SemanticRuntimeChannelState::default()
        };
        let invocation_id = SemanticInvocationId::new(22).expect("invocation");
        state
            .dispatch(
                invocation(22, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            )
            .unwrap_or_else(|_| panic!("dispatch"));
        let (actions, matched) =
            state.timeout(SemanticInvocationId::new(21).expect("different invocation"));
        assert!(!matched);
        assert!(actions.completion.is_none());
        assert!(state.pending.is_some());

        let (actions, matched) = state.timeout(invocation_id);
        assert!(matched);
        assert!(matches!(
            actions.completion.expect("timeout completion").outcome,
            Err(AgentSemanticRuntimeFailure::TimedOut)
        ));
        assert_eq!(state.phase, DocumentPhase::Failed);
        assert!(state.pending.is_none());
        assert!(state
            .dispatch(
                invocation(23, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            )
            .is_err());
    }
}
