#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

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

#[cfg(feature = "native-agentic-semantic-probe")]
#[path = "agentic_semantic_program_probe.rs"]
mod program_probe;
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyObject, runtime::NSObject};
use objc2::{DefinedClass as _, MainThreadOnly, Message as _};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSString, NSUTF8StringEncoding};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandlerWithReply, WKUserContentController,
    WKUserScript, WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration,
};
#[cfg(any(not(feature = "native-agentic-semantic-probe"), test))]
use zephium_agentic::SEMANTIC_RUNTIME_PROGRAM;
use zephium_agentic::{
    SemanticActionAttemptId, SemanticActionRuntimeEvidence, SemanticActionRuntimeInvocation,
    SemanticActionRuntimeResultError, SemanticRuntimeInvocation, SemanticRuntimeResultError,
    SemanticSnapshot, MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES,
    MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS, SEMANTIC_RUNTIME_CHANNEL_ACK,
    SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED, SEMANTIC_RUNTIME_CHANNEL_NAME,
    SEMANTIC_RUNTIME_CHANNEL_PARK, SEMANTIC_RUNTIME_CHANNEL_PARKED, SEMANTIC_RUNTIME_CHANNEL_PULL,
    SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX, SEMANTIC_RUNTIME_CHANNEL_STOP,
};

const SEMANTIC_RUNTIME_WORLD_NAME_PREFIX: &str = "zephium-semantic-runtime-v1-";
const SEMANTIC_RUNTIME_FIXED_ERROR: &str = "zephium semantic channel refused";
static NEXT_SEMANTIC_RUNTIME_WORLD: AtomicU64 = AtomicU64::new(1);

type ReplyBlock = RcBlock<dyn Fn(*mut AnyObject, *mut NSString)>;
type SemanticCompletion = Box<dyn FnOnce(Result<SemanticSnapshot, AgentSemanticRuntimeFailure>)>;
type SemanticActionCompletion =
    Box<dyn FnOnce(Result<SemanticActionRuntimeEvidence, AgentSemanticActionRuntimeFailure>)>;
type SemanticParkCompletion = Box<dyn FnOnce(bool)>;

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

/// Closed terminal failure for one action-target revalidation invocation.
#[derive(Debug)]
pub(crate) enum AgentSemanticActionRuntimeFailure {
    Dispatch(AgentSemanticRuntimeDispatchError),
    Cancelled,
    DocumentReplaced,
    RendererLost,
    TimedOut,
    Retired,
    Transport,
    Result(SemanticActionRuntimeResultError),
}

#[derive(Clone, Copy)]
enum RuntimeChannelFailure {
    Cancelled,
    DocumentReplaced,
    RendererLost,
    TimedOut,
    Retired,
    Transport,
}

impl RuntimeChannelFailure {
    const fn observation(self) -> AgentSemanticRuntimeFailure {
        match self {
            Self::Cancelled => AgentSemanticRuntimeFailure::Cancelled,
            Self::DocumentReplaced => AgentSemanticRuntimeFailure::DocumentReplaced,
            Self::RendererLost => AgentSemanticRuntimeFailure::RendererLost,
            Self::TimedOut => AgentSemanticRuntimeFailure::TimedOut,
            Self::Retired => AgentSemanticRuntimeFailure::Retired,
            Self::Transport => AgentSemanticRuntimeFailure::Transport,
        }
    }

    const fn action(self) -> AgentSemanticActionRuntimeFailure {
        match self {
            Self::Cancelled => AgentSemanticActionRuntimeFailure::Cancelled,
            Self::DocumentReplaced => AgentSemanticActionRuntimeFailure::DocumentReplaced,
            Self::RendererLost => AgentSemanticActionRuntimeFailure::RendererLost,
            Self::TimedOut => AgentSemanticActionRuntimeFailure::TimedOut,
            Self::Retired => AgentSemanticActionRuntimeFailure::Retired,
            Self::Transport => AgentSemanticActionRuntimeFailure::Transport,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentPhase {
    Loading,
    Ready,
    Parking,
    Parked,
    // Admission is permanently closed. Only the exact handed-off action may
    // return evidence; this phase never grants document authority.
    AuthorityRevoked,
    RendererLost,
    ExhaustionNoticePending,
    Exhausted,
    Failed,
    Retired,
}

enum PendingInvocation {
    Observation {
        invocation: SemanticRuntimeInvocation,
        completion: SemanticCompletion,
    },
    Action {
        invocation: SemanticActionRuntimeInvocation,
        completion: SemanticActionCompletion,
        authority: Option<Box<dyn Fn() -> bool>>,
    },
}

impl PendingInvocation {
    fn as_str(&self) -> &str {
        match self {
            Self::Observation { invocation, .. } => invocation.as_str(),
            Self::Action { invocation, .. } => invocation.as_str(),
        }
    }

    fn matches_observation(&self, invocation: zephium_agentic::SemanticInvocationId) -> bool {
        matches!(self, Self::Observation { invocation: current, .. } if current.invocation() == invocation)
    }

    fn matches_action(&self, attempt: SemanticActionAttemptId) -> bool {
        matches!(self, Self::Action { invocation, .. } if invocation.attempt() == attempt)
    }
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

enum CompletionAction {
    Observation {
        completion: SemanticCompletion,
        outcome: Result<Box<SemanticSnapshot>, AgentSemanticRuntimeFailure>,
    },
    Action {
        completion: SemanticActionCompletion,
        outcome: Result<SemanticActionRuntimeEvidence, AgentSemanticActionRuntimeFailure>,
    },
}

#[cfg(test)]
impl CompletionAction {
    fn observation_outcome(self) -> Result<SemanticSnapshot, AgentSemanticRuntimeFailure> {
        match self {
            Self::Observation { outcome, .. } => outcome.map(|snapshot| *snapshot),
            Self::Action { .. } => panic!("expected observation completion"),
        }
    }
}

#[derive(Default)]
struct ChannelActions {
    first_reply: Option<ReplyAction>,
    second_reply: Option<ReplyAction>,
    completion: Option<CompletionAction>,
    park_completion: Option<(SemanticParkCompletion, bool)>,
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
    // Lifetime evidence only: the exact page command returned an uncertain
    // applied effect. No read/action admission consults this field.
    settling_action: Option<SemanticActionAttemptId>,
    park_completion: Option<SemanticParkCompletion>,
    completed_invocations: u16,
    #[cfg(feature = "native-agentic-semantic-probe")]
    prepared_fill: Option<program_probe::PreparedFill>,
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
            settling_action: None,
            park_completion: None,
            completed_invocations: 0,
            #[cfg(feature = "native-agentic-semantic-probe")]
            prepared_fill: None,
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

    fn dispatch_observation(
        &mut self,
        invocation: SemanticRuntimeInvocation,
        completion: SemanticCompletion,
    ) -> Result<ChannelActions, (AgentSemanticRuntimeDispatchError, SemanticCompletion)> {
        if let Some(failure) = self.admission_failure() {
            return Err((failure, completion));
        }
        self.settling_action = None;
        self.pending = Some(PendingInvocation::Observation {
            invocation,
            completion,
        });
        Ok(self.prepare_pump())
    }

    fn dispatch_action(
        &mut self,
        invocation: SemanticActionRuntimeInvocation,
        completion: SemanticActionCompletion,
        authority: Option<Box<dyn Fn() -> bool>>,
    ) -> Result<ChannelActions, (AgentSemanticRuntimeDispatchError, SemanticActionCompletion)> {
        if let Some(failure) = self.admission_failure() {
            return Err((failure, completion));
        }
        self.settling_action = None;
        self.pending = Some(PendingInvocation::Action {
            invocation,
            completion,
            authority,
        });
        Ok(self.prepare_pump())
    }

    fn admission_failure(&mut self) -> Option<AgentSemanticRuntimeDispatchError> {
        if self.expected_view.is_none() {
            return Some(AgentSemanticRuntimeDispatchError::NotReady);
        }
        match self.phase {
            DocumentPhase::Ready => {}
            DocumentPhase::Loading => {
                return Some(AgentSemanticRuntimeDispatchError::NotReady);
            }
            DocumentPhase::ExhaustionNoticePending | DocumentPhase::Exhausted => {
                return Some(AgentSemanticRuntimeDispatchError::Exhausted);
            }
            DocumentPhase::AuthorityRevoked
            | DocumentPhase::Parking
            | DocumentPhase::Parked
            | DocumentPhase::RendererLost
            | DocumentPhase::Failed
            | DocumentPhase::Retired => {
                return Some(AgentSemanticRuntimeDispatchError::Retired);
            }
        }
        if self.pending.is_some() || self.awaiting_result {
            return Some(AgentSemanticRuntimeDispatchError::Busy);
        }
        if self.completed_invocations >= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS {
            self.phase = DocumentPhase::Exhausted;
            return Some(AgentSemanticRuntimeDispatchError::Exhausted);
        }
        None
    }

    fn document_committed(&mut self) -> ChannelActions {
        let mut actions = ChannelActions::default();
        if self.phase == DocumentPhase::Loading {
            self.phase = DocumentPhase::Ready;
        } else {
            actions = self.invalidate_current(RuntimeChannelFailure::DocumentReplaced);
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
        let actions = self.invalidate_current(RuntimeChannelFailure::DocumentReplaced);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Loading;
            self.completed_invocations = 0;
            self.active_world = None;
        }
        actions
    }

    fn begin_park(
        &mut self,
        completion: SemanticParkCompletion,
    ) -> Result<ChannelActions, SemanticParkCompletion> {
        if self.phase != DocumentPhase::Ready
            || self.pending.is_some()
            || self.awaiting_result
            || self.settling_action.is_some()
            || self.park_completion.is_some()
        {
            return Err(completion);
        }
        let Some(pull) = self.pull.take() else {
            return Err(completion);
        };
        self.phase = DocumentPhase::Parking;
        self.park_completion = Some(completion);
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(pull, SEMANTIC_RUNTIME_CHANNEL_PARK));
        Ok(actions)
    }

    fn reactivate(&mut self, world: usize) -> Result<ChannelActions, ()> {
        if self.phase != DocumentPhase::Parked
            || self.pending.is_some()
            || self.awaiting_result
            || self.pull.is_some()
            || self.park_completion.is_some()
        {
            return Err(());
        }
        self.phase = DocumentPhase::Loading;
        self.completed_invocations = 0;
        self.active_world = Some(world);
        Ok(ChannelActions::default())
    }

    fn registration_failed(&mut self) -> ChannelActions {
        let mut actions = self.invalidate_current(RuntimeChannelFailure::Transport);
        self.active_world = None;
        self.phase = DocumentPhase::Failed;
        actions.invariant_failed = true;
        actions
    }

    fn renderer_lost(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(RuntimeChannelFailure::RendererLost);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::RendererLost;
        }
        actions
    }

    fn cancel(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(RuntimeChannelFailure::Cancelled);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Failed;
        }
        actions
    }

    fn revoke_document_authority(&mut self) -> ChannelActions {
        if !matches!(
            self.phase,
            DocumentPhase::Ready | DocumentPhase::AuthorityRevoked
        ) {
            return self.cancel();
        }
        let handed_off =
            self.awaiting_result && matches!(self.pending, Some(PendingInvocation::Action { .. }));
        let mut actions = if handed_off || self.settling_action.is_some() {
            ChannelActions::default()
        } else {
            self.invalidate_current(RuntimeChannelFailure::DocumentReplaced)
        };
        // Even a preparation reply belonging to the original action cannot
        // release more work after drift. Preserve only its eventual terminal.
        #[cfg(feature = "native-agentic-semantic-probe")]
        if let Some(mut prepared) = self.prepared_fill.take() {
            if let Some(reply) = prepared.stop() {
                actions.push_reply(reply);
            }
        }
        if let Some(pull) = self.pull.take() {
            actions.push_reply(ReplyAction::success(pull, SEMANTIC_RUNTIME_CHANNEL_STOP));
        }
        self.phase = DocumentPhase::AuthorityRevoked;
        actions
    }

    fn draining_action(&self, attempt: SemanticActionAttemptId) -> bool {
        self.phase == DocumentPhase::AuthorityRevoked
            && self.awaiting_result
            && self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.matches_action(attempt))
    }

    fn settling_action(&self, attempt: SemanticActionAttemptId) -> bool {
        matches!(
            self.phase,
            DocumentPhase::Ready | DocumentPhase::AuthorityRevoked
        ) && self.settling_action == Some(attempt)
            && !self.awaiting_result
            && self.pending.is_none()
    }

    fn revoked_settling_action(&self, attempt: SemanticActionAttemptId) -> bool {
        self.phase == DocumentPhase::AuthorityRevoked && self.settling_action(attempt)
    }

    fn timeout(
        &mut self,
        invocation: zephium_agentic::SemanticInvocationId,
    ) -> (ChannelActions, bool) {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| !pending.matches_observation(invocation))
        {
            return (ChannelActions::default(), false);
        }
        let actions = self.invalidate_current(RuntimeChannelFailure::TimedOut);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Failed;
        }
        (actions, true)
    }

    fn timeout_action(&mut self, attempt: SemanticActionAttemptId) -> (ChannelActions, bool) {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| !pending.matches_action(attempt))
        {
            return (ChannelActions::default(), false);
        }
        let actions = self.invalidate_current(RuntimeChannelFailure::TimedOut);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Failed;
        }
        (actions, true)
    }

    fn retire(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(RuntimeChannelFailure::Retired);
        self.active_world = None;
        self.phase = DocumentPhase::Retired;
        actions
    }

    fn fail_transport(&mut self, current: Option<ReplyBlock>) -> ChannelActions {
        let mut actions = self.invalidate_current(RuntimeChannelFailure::Transport);
        if let Some(reply) = current {
            actions.push_reply(ReplyAction::error(reply));
        }
        self.phase = DocumentPhase::Failed;
        actions.invariant_failed = true;
        actions
    }

    fn invalidate_current(&mut self, failure: RuntimeChannelFailure) -> ChannelActions {
        self.settling_action = None;
        let mut actions = ChannelActions::default();
        #[cfg(feature = "native-agentic-semantic-probe")]
        if let Some(mut prepared) = self.prepared_fill.take() {
            if let Some(reply) = prepared.stop() {
                actions.push_reply(reply);
            }
        }
        if let Some(pull) = self.pull.take() {
            actions.push_reply(ReplyAction::success(pull, SEMANTIC_RUNTIME_CHANNEL_STOP));
        }
        self.awaiting_result = false;
        if let Some(completion) = self.park_completion.take() {
            actions.park_completion = Some((completion, false));
        }
        if let Some(pending) = self.pending.take() {
            actions.completion = Some(match pending {
                PendingInvocation::Observation { completion, .. } => {
                    CompletionAction::Observation {
                        completion,
                        outcome: Err(failure.observation()),
                    }
                }
                PendingInvocation::Action { completion, .. } => CompletionAction::Action {
                    completion,
                    outcome: Err(failure.action()),
                },
            });
        }
        actions
    }

    fn on_message(&mut self, body: &str, reply: ReplyBlock) -> ChannelActions {
        #[cfg(feature = "native-agentic-semantic-probe")]
        if body == program_probe::PREPARE_MESSAGE && program_probe::preparation_selected() {
            return self.on_fill_preparation(reply);
        }
        if body == SEMANTIC_RUNTIME_CHANNEL_PULL {
            return self.on_pull(reply);
        }
        if body == SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED {
            return self.on_exhausted(reply);
        }
        if body == SEMANTIC_RUNTIME_CHANNEL_PARKED {
            return self.on_parked(reply);
        }
        if let Some(result) = body.strip_prefix(SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX) {
            return self.on_result(result.as_bytes(), reply);
        }
        self.fail_transport(Some(reply))
    }

    fn on_pull(&mut self, reply: ReplyBlock) -> ChannelActions {
        if matches!(
            self.phase,
            DocumentPhase::AuthorityRevoked
                | DocumentPhase::Parking
                | DocumentPhase::Parked
                | DocumentPhase::RendererLost
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

    fn on_parked(&mut self, reply: ReplyBlock) -> ChannelActions {
        if self.phase != DocumentPhase::Parking
            || self.pending.is_some()
            || self.awaiting_result
            || self.pull.is_some()
            || self.settling_action.is_some()
        {
            return self.fail_transport(Some(reply));
        }
        let Some(completion) = self.park_completion.take() else {
            return self.fail_transport(Some(reply));
        };
        self.completed_invocations = 0;
        self.phase = DocumentPhase::Parked;
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(reply, SEMANTIC_RUNTIME_CHANNEL_ACK));
        actions.park_completion = Some((completion, true));
        actions
    }

    fn on_result(&mut self, bytes: &[u8], reply: ReplyBlock) -> ChannelActions {
        #[cfg(feature = "native-agentic-semantic-probe")]
        if self
            .prepared_fill
            .as_ref()
            .is_some_and(program_probe::PreparedFill::waiting)
        {
            return self.fail_transport(Some(reply));
        }
        if !self.awaiting_result {
            return self.fail_transport(Some(reply));
        }
        let Some(pending) = self.pending.take() else {
            return self.fail_transport(Some(reply));
        };
        self.awaiting_result = false;
        #[cfg(feature = "native-agentic-semantic-probe")]
        {
            self.prepared_fill = None;
        }
        let Some(completed) = self.completed_invocations.checked_add(1) else {
            let mut actions = self.fail_transport(Some(reply));
            actions.completion = Some(match pending {
                PendingInvocation::Observation { completion, .. } => {
                    CompletionAction::Observation {
                        completion,
                        outcome: Err(AgentSemanticRuntimeFailure::Transport),
                    }
                }
                PendingInvocation::Action { completion, .. } => CompletionAction::Action {
                    completion,
                    outcome: Err(AgentSemanticActionRuntimeFailure::Transport),
                },
            });
            return actions;
        };
        self.completed_invocations = completed;
        if completed == MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS
            && self.phase == DocumentPhase::Ready
        {
            self.phase = DocumentPhase::ExhaustionNoticePending;
        }

        let (completion, recoverable) = match pending {
            PendingInvocation::Observation {
                invocation,
                completion,
            } => {
                let outcome = invocation
                    .decode_result(bytes)
                    .map(Box::new)
                    .map_err(AgentSemanticRuntimeFailure::Result);
                let recoverable = matches!(
                    &outcome,
                    Ok(_)
                        | Err(AgentSemanticRuntimeFailure::Result(
                            SemanticRuntimeResultError::Runtime(_)
                        ))
                );
                (
                    CompletionAction::Observation {
                        completion,
                        outcome,
                    },
                    recoverable,
                )
            }
            PendingInvocation::Action {
                invocation,
                completion,
                ..
            } => {
                #[cfg(feature = "native-agentic-semantic-probe")]
                let bytes = program_probe::normalize_diagnostic(bytes);
                let outcome = invocation
                    .decode_result(bytes)
                    .map_err(AgentSemanticActionRuntimeFailure::Result);
                self.settling_action = matches!(
                    &outcome,
                    Err(AgentSemanticActionRuntimeFailure::Result(
                        SemanticActionRuntimeResultError::Runtime(
                            fault
                        )
                    )) if super::semantic_action::map_runtime_fault(*fault)
                        == zephium_agentic::SemanticActionNativeFailure::AppliedUnverified
                )
                .then_some(invocation.attempt());
                #[cfg(feature = "native-agentic-semantic-probe")]
                if let Err(AgentSemanticActionRuntimeFailure::Result(
                    SemanticActionRuntimeResultError::Runtime(fault),
                )) = &outcome
                {
                    program_probe::record_fault(fault);
                }
                let recoverable = matches!(
                    &outcome,
                    Ok(_)
                        | Err(AgentSemanticActionRuntimeFailure::Result(
                            SemanticActionRuntimeResultError::Runtime(_)
                        ))
                );
                (
                    CompletionAction::Action {
                        completion,
                        outcome,
                    },
                    recoverable,
                )
            }
        };
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(
            reply,
            if recoverable && self.phase != DocumentPhase::AuthorityRevoked {
                SEMANTIC_RUNTIME_CHANNEL_ACK
            } else {
                SEMANTIC_RUNTIME_CHANNEL_STOP
            },
        ));
        actions.completion = Some(completion);
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
        // A retained recipe can wait for the page's next pull. Recheck the
        // native owner at the actual handoff, before exposing any effect bytes.
        if self.pending.as_ref().is_some_and(|pending| {
            matches!(pending,
                PendingInvocation::Action { authority: Some(authority), .. } if !authority()
            )
        }) {
            return self.cancel();
        }
        let request = self
            .pending
            .as_ref()
            .map(|pending| pending.as_str().to_owned().into_boxed_str());
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
    pub(crate) fn park(&self, completion: impl FnOnce(bool) + 'static) -> Result<(), ()> {
        let completion: SemanticParkCompletion = Box::new(completion);
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return Err(());
        };
        let actions = state.begin_park(completion).map_err(|_| ())?;
        drop(state);
        self.execute(actions);
        Ok(())
    }

    fn reactivate(&self, world: &WKContentWorld) -> Result<(), ()> {
        let actions = self
            .state
            .try_borrow_mut()
            .map_err(|_| ())?
            .reactivate(std::ptr::from_ref(world).addr())?;
        self.execute(actions);
        Ok(())
    }

    /// Close all future use while retaining only an action already handed to
    /// this exact content world. Its original timeout/cancel owners still win.
    pub(crate) fn revoke_document_authority(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::revoke_document_authority);
        self.execute(actions);
    }

    pub(crate) fn draining_action(&self, attempt: SemanticActionAttemptId) -> bool {
        self.state
            .try_borrow()
            .is_ok_and(|state| state.draining_action(attempt))
    }
    /// Passive rendering eligibility only, never document authority.
    pub(crate) fn settling_action(&self, attempt: SemanticActionAttemptId) -> bool {
        self.state
            .try_borrow()
            .is_ok_and(|state| state.settling_action(attempt))
    }
    pub(crate) fn revoked_settling_action(&self, attempt: SemanticActionAttemptId) -> bool {
        self.state
            .try_borrow()
            .is_ok_and(|state| state.revoked_settling_action(attempt))
    }
    #[cfg(feature = "native-agentic-semantic-probe")]
    pub(crate) fn poll_prepared_fill(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::poll_fill_preparation);
        self.execute(actions);
    }
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
            Ok(mut state) => state.dispatch_observation(invocation, completion),
            Err(_) => Err((AgentSemanticRuntimeDispatchError::Busy, completion)),
        };
        match dispatched {
            Ok(actions) => {
                self.execute(actions);
                Ok(())
            }
            Err((failure, completion)) => {
                invoke_completion(
                    CompletionAction::Observation {
                        completion,
                        outcome: Err(AgentSemanticRuntimeFailure::Dispatch(failure)),
                    },
                    self.on_callback_panic.as_ref(),
                );
                Err(failure)
            }
        }
    }

    pub(crate) fn dispatch_action_guarded(
        &self,
        invocation: SemanticActionRuntimeInvocation,
        authority: Option<Box<dyn Fn() -> bool>>,
        completion: impl FnOnce(Result<SemanticActionRuntimeEvidence, AgentSemanticActionRuntimeFailure>)
            + 'static,
    ) -> Result<(), AgentSemanticRuntimeDispatchError> {
        let completion: SemanticActionCompletion = Box::new(completion);
        let dispatched = match self.state.try_borrow_mut() {
            Ok(mut state) => state.dispatch_action(invocation, completion, authority),
            Err(_) => Err((AgentSemanticRuntimeDispatchError::Busy, completion)),
        };
        match dispatched {
            Ok(actions) => {
                self.execute(actions);
                Ok(())
            }
            Err((failure, completion)) => {
                invoke_completion(
                    CompletionAction::Action {
                        completion,
                        outcome: Err(AgentSemanticActionRuntimeFailure::Dispatch(failure)),
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

    pub(crate) fn timeout_action(&self, attempt: SemanticActionAttemptId) -> bool {
        let (actions, matched) = match self.state.try_borrow_mut() {
            Ok(mut state) => state.timeout_action(attempt),
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
            && (state.phase != DocumentPhase::AuthorityRevoked
                || (state.pull.is_none()
                    && (!pending
                        || (state.awaiting_result
                            && matches!(state.pending, Some(PendingInvocation::Action { .. }))))))
            && (!state.awaiting_result || (pending && state.pull.is_none()))
            && (state.pull.is_none() || (!state.awaiting_result && !pending))
            && (!matches!(
                state.phase,
                DocumentPhase::RendererLost
                    | DocumentPhase::Parking
                    | DocumentPhase::Parked
                    | DocumentPhase::ExhaustionNoticePending
                    | DocumentPhase::Exhausted
                    | DocumentPhase::Failed
                    | DocumentPhase::Retired
            ) || (!pending && !state.awaiting_result && state.pull.is_none()));
        valid.then_some(pending)
    }

    pub(crate) fn parked_for_history(&self) -> bool {
        self.state.try_borrow().is_ok_and(|state| {
            state.phase == DocumentPhase::Parked
                && state.pending.is_none()
                && !state.awaiting_result
                && state.pull.is_none()
                && state.park_completion.is_none()
        })
    }

    // Private debug evidence only. Addresses never leave the in-memory witness.
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn witness_identity(&self) -> Option<(usize, usize, u16)> {
        if self.pending_for_audit() != Some(false) {
            return None;
        }
        let state = self.state.try_borrow().ok()?;
        if state.phase != DocumentPhase::Ready {
            return None;
        }
        Some((
            state.expected_view?,
            state.active_world?,
            state.completed_invocations,
        ))
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
        let mut park_completion = actions.park_completion.take();
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
            let (merged, duplicate) = reconcile_failed_reply_park_completion(
                park_completion.take(),
                failure.park_completion.take(),
            );
            park_completion = merged;
            if duplicate {
                invoke_unit_callback(
                    self.on_invariant_failure.as_ref(),
                    self.on_callback_panic.as_ref(),
                );
            }
        }
        if let Some(completion) = actions.completion.take() {
            invoke_completion(completion, self.on_callback_panic.as_ref());
        }
        if let Some((completion, parked)) = park_completion {
            if std::panic::catch_unwind(AssertUnwindSafe(|| completion(parked))).is_err() {
                invoke_unit_callback(
                    self.on_invariant_failure.as_ref(),
                    self.on_callback_panic.as_ref(),
                );
            }
        }
    }
}

fn reconcile_failed_reply_park_completion(
    primary: Option<(SemanticParkCompletion, bool)>,
    failure: Option<(SemanticParkCompletion, bool)>,
) -> (Option<(SemanticParkCompletion, bool)>, bool) {
    match (primary, failure) {
        (Some((completion, _)), None) | (None, Some((completion, _))) => {
            (Some((completion, false)), false)
        }
        (Some((completion, _)), Some(_)) => (Some((completion, false)), true),
        (None, None) => (None, false),
    }
}

fn invoke_completion(completion: CompletionAction, on_panic: &dyn Fn()) {
    if std::panic::catch_unwind(AssertUnwindSafe(|| match completion {
        CompletionAction::Observation {
            completion,
            outcome,
        } => completion(outcome.map(|snapshot| *snapshot)),
        CompletionAction::Action {
            completion,
            outcome,
        } => completion(outcome),
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

    // SAFETY: `define_class!` fixes `NSObject` as this class's superclass and
    // objc2 initializes the declared ivars before exposing an instance.
    unsafe impl NSObjectProtocol for SemanticMessageHandler {}

    // SAFETY: the registered selector and Rust parameters exactly implement
    // WebKit's generated reply-handler protocol. `MainThreadOnly` prevents the
    // object from crossing threads, and callback state is retained in ivars.
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
            // SAFETY: WebKit supplied live protocol callback objects for this
            // exact selector; objc2 retains each object returned by the four
            // property messages for the duration of the checks below.
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
            // SAFETY: `frame` is the live retained WKFrameInfo obtained from
            // this callback message and this MainThreadOnly handler is running
            // on WebKit's main-thread delivery path.
            let (frame_webview, is_main_frame) = unsafe { (frame.webView(), frame.isMainFrame()) };
            let frame_view_matches = frame_webview
                .as_ref()
                .is_some_and(|view| std::ptr::from_ref(&**view).addr() == expected_view);
            if !view_matches || !frame_view_matches || !is_main_frame {
                ivars.channel.reject(reply, true);
                return;
            }
            // SAFETY: `message` remains live for this protocol callback; objc2
            // retains its Objective-C body before the type and size checks.
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

struct SemanticRuntimeEpochs {
    active: Option<SemanticRuntimeEpochRegistration>,
    active_runtime: Option<super::agent_history::AgentDocumentRuntimeId>,
    parked: Vec<(
        super::agent_history::AgentDocumentRuntimeId,
        SemanticRuntimeEpochRegistration,
    )>,
}

fn attach_semantic_runtime_epoch(
    controller: &WKUserContentController,
    handler_name: &NSString,
    epoch: &SemanticRuntimeEpochRegistration,
) -> Result<(), ()> {
    let _mtm = MainThreadMarker::new().ok_or(())?;
    let protocol_handler = objc2::runtime::ProtocolObject::from_ref(&*epoch.handler);
    let attached = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        controller.addScriptMessageHandlerWithReply_contentWorld_name(
            protocol_handler,
            &epoch.world,
            handler_name,
        );
        controller.addUserScript(&epoch.script);
    }))
    .is_ok();
    if !attached {
        let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&epoch.world));
        return Err(());
    }
    // SAFETY: main-thread access is proven above and objc2 retains the bounded
    // controller inventory for this exact comparison.
    let scripts = unsafe { controller.userScripts() };
    if scripts.count() != 1
        || Retained::as_ptr(&scripts.objectAtIndex(0)) != Retained::as_ptr(&epoch.script)
    {
        let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&epoch.world));
        return Err(());
    }
    Ok(())
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
    // SAFETY: `mtm` proves main-thread creation and `name` is a live retained
    // NSString. Objective-C exceptions are contained at this boundary.
    let world = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        WKContentWorld::worldWithName(&name, mtm)
    }))
    .map_err(|_| ())?;
    // SAFETY: `world` is the retained object returned above and access remains
    // on the main thread proven by `mtm`.
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
    if MainThreadMarker::new().is_none() {
        return false;
    }
    // SAFETY: the marker check above proves main-thread access; controller,
    // handler name, and optional world are live retained objects. Exceptions
    // are caught and failure leaves the caller in fail-closed cleanup.
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
    // SAFETY: `controller` remains live on the verified main thread; objc2
    // retains the returned script array for this bounded inventory check.
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
    // SAFETY: `handler` is a freshly allocated instance of the declared class
    // with fully initialized ivars; `init` is NSObject's designated initializer.
    let handler: Retained<SemanticMessageHandler> = unsafe { msg_send![super(handler), init] };
    let protocol_handler = objc2::runtime::ProtocolObject::from_ref(&*handler);
    // SAFETY: `mtm` proves main-thread registration and every Objective-C
    // argument is retained for this call. WebKit retains the protocol handler;
    // exceptions are caught and trigger complete controller cleanup.
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

    #[cfg(not(feature = "native-agentic-semantic-probe"))]
    let source = NSString::from_str(SEMANTIC_RUNTIME_PROGRAM.source());
    #[cfg(feature = "native-agentic-semantic-probe")]
    let source = NSString::from_str(&program_probe::source());
    // SAFETY: `mtm` proves main-thread allocation; source and content world are
    // live retained values, and the fixed enum/bool arguments match WebKit's
    // initializer contract. Objective-C exceptions are contained.
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
    // SAFETY: controller and script are live retained main-thread objects;
    // WebKit retains the script and any exception is converted to refusal.
    let added = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        controller.addUserScript(&script);
    }))
    .is_ok();
    if !added {
        let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
        return Err(());
    }
    if channel.bind_world(&world).is_err() {
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
    epochs: Rc<RefCell<SemanticRuntimeEpochs>>,
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
        // SAFETY: `mtm` proves main-thread access and `configuration` is a live
        // retained configuration supplied by the construction path. objc2
        // retains the returned controller and script inventory.
        let controller = unsafe { configuration.userContentController() };
        // SAFETY: the retained controller remains live on the same main thread.
        if unsafe { controller.userScripts() }.count() != 0 {
            return Err(());
        }
        let handler_name = NSString::from_str(SEMANTIC_RUNTIME_CHANNEL_NAME);
        let channel = AgentSemanticRuntimeController::new(on_invariant_failure, on_callback_panic);
        let active = install_semantic_runtime_epoch(&controller, &handler_name, &channel, mtm)?;

        let registration = Self {
            controller,
            handler_name,
            epochs: Rc::new(RefCell::new(SemanticRuntimeEpochs {
                active: Some(active),
                active_runtime: None,
                parked: Vec::new(),
            })),
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

    pub(crate) fn bind_active_runtime(
        &mut self,
        runtime: super::agent_history::AgentDocumentRuntimeId,
    ) -> Result<(), ()> {
        if self.retired {
            return Err(());
        }
        let mut epochs = self.epochs.try_borrow_mut().map_err(|_| ())?;
        if epochs.active.is_none() || epochs.active_runtime.is_some() {
            return Err(());
        }
        epochs.active_runtime = Some(runtime);
        Ok(())
    }

    pub(crate) fn reset_history_authority(&mut self) -> Result<(), ()> {
        if self.retired || self.channel.pending_for_audit() != Some(false) {
            return Err(());
        }
        let mut epochs = self.epochs.try_borrow_mut().map_err(|_| ())?;
        if epochs.active.is_none() {
            return Err(());
        }
        epochs.active_runtime = None;
        epochs.parked.clear();
        Ok(())
    }

    pub(crate) fn active_parked(&self) -> bool {
        if self.retired || !self.channel.parked_for_history() {
            return false;
        }
        self.epochs.try_borrow().is_ok_and(|epochs| {
            epochs.active.is_none() && epochs.active_runtime.is_none() && !epochs.parked.is_empty()
        })
    }

    pub(crate) fn reactivate_runtime(
        &mut self,
        runtime: super::agent_history::AgentDocumentRuntimeId,
    ) -> Result<(), ()> {
        if self.retired || !self.channel.parked_for_history() {
            return Err(());
        }
        let epoch = {
            let mut epochs = self.epochs.try_borrow_mut().map_err(|_| ())?;
            if epochs.active.is_some() || epochs.active_runtime.is_some() {
                return Err(());
            }
            let index = epochs
                .parked
                .iter()
                .position(|(candidate, _)| *candidate == runtime)
                .ok_or(())?;
            epochs.parked.remove(index).1
        };
        if attach_semantic_runtime_epoch(&self.controller, &self.handler_name, &epoch).is_err()
            || self.channel.reactivate(&epoch.world).is_err()
        {
            let _ = clear_semantic_runtime_controller(
                &self.controller,
                &self.handler_name,
                Some(&epoch.world),
            );
            self.channel.registration_failed();
            return Err(());
        }
        let mut epochs = self.epochs.try_borrow_mut().map_err(|_| ())?;
        epochs.active = Some(epoch);
        epochs.active_runtime = Some(runtime);
        Ok(())
    }

    /// Asks the exact active document to discard every semantic capability,
    /// then detaches its handler/script and retains the opaque epoch for a
    /// possible exact native-history traversal. The completion is called only
    /// after the page acknowledged that it has no pending reply or node state.
    pub(crate) fn park_active(
        &mut self,
        completion: impl FnOnce(bool) + 'static,
    ) -> Result<(), ()> {
        if self.retired {
            return Err(());
        }
        {
            let epochs = self.epochs.try_borrow().map_err(|_| ())?;
            if epochs.active.is_none() || epochs.active_runtime.is_none() {
                return Err(());
            }
        }
        let controller = self.controller.clone();
        let handler_name = self.handler_name.clone();
        let epochs = self.epochs.clone();
        self.channel.park(move |acknowledged| {
            let parked = if acknowledged {
                epochs.try_borrow_mut().is_ok_and(|mut epochs| {
                    let Some(runtime) = epochs.active_runtime.take() else {
                        return false;
                    };
                    let Some(active) = epochs.active.take() else {
                        return false;
                    };
                    if !clear_semantic_runtime_controller(
                        &controller,
                        &handler_name,
                        Some(&active.world),
                    ) {
                        return false;
                    }
                    if epochs.parked.len() >= super::agent_history::MAX_AGENT_HISTORY_ENTRIES {
                        return false;
                    }
                    epochs.parked.push((runtime, active));
                    true
                })
            } else {
                false
            };
            completion(parked);
        })
    }

    /// Revokes the old document world and installs the immutable program in a
    /// fresh one before native navigation can begin.
    pub(crate) fn prepare_document_load(&mut self) -> Result<(), ()> {
        if self.retired {
            return Err(());
        }
        self.channel.begin_document_load();
        let old = {
            let mut epochs = self.epochs.try_borrow_mut().map_err(|_| ())?;
            if epochs.active_runtime.is_some() {
                self.channel.registration_failed();
                return Err(());
            }
            epochs.active.take()
        };
        if let Some(old) = old {
            if !clear_semantic_runtime_controller(
                &self.controller,
                &self.handler_name,
                Some(&old.world),
            ) {
                self.channel.registration_failed();
                return Err(());
            }
        }
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
                if self
                    .epochs
                    .try_borrow_mut()
                    .map_err(|_| ())?
                    .active
                    .replace(active)
                    .is_some()
                {
                    self.channel.registration_failed();
                    return Err(());
                }
                if self.attest_controller().is_ok() {
                    Ok(())
                } else {
                    let mut epochs = self.epochs.try_borrow_mut().map_err(|_| ())?;
                    let active = epochs.active.take();
                    let world = active.as_ref().map(|active| &*active.world);
                    let _ = clear_semantic_runtime_controller(
                        &self.controller,
                        &self.handler_name,
                        world,
                    );
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
        let _mtm = MainThreadMarker::new().ok_or(())?;
        // SAFETY: the marker above proves main-thread access; `configuration`
        // is live and objc2 retains its returned content controller.
        let actual_controller = unsafe { configuration.userContentController() };
        if Retained::as_ptr(&actual_controller) != Retained::as_ptr(&self.controller) {
            return Err(());
        }
        self.attest_controller()
    }

    fn attest_controller(&self) -> Result<(), ()> {
        let _mtm = MainThreadMarker::new().ok_or(())?;
        let epochs = self.epochs.try_borrow().map_err(|_| ())?;
        let active = epochs.active.as_ref().ok_or(())?;
        if self.channel.world_matches(&active.world) != Ok(true) {
            return Err(());
        }
        // SAFETY: the marker above proves main-thread access; the retained
        // controller remains live and objc2 retains its script inventory.
        let scripts = unsafe { self.controller.userScripts() };
        if scripts.count() != 1 {
            return Err(());
        }
        let script = scripts.objectAtIndex(0);
        // SAFETY: the count check proves index zero exists, `script` is retained
        // by objc2, and all WebKit property reads remain on the main thread.
        let (source, injection_time, main_frame_only) = unsafe {
            (
                script.source(),
                script.injectionTime(),
                script.isForMainFrameOnly(),
            )
        };
        #[cfg(not(feature = "native-agentic-semantic-probe"))]
        let source_mismatch = source.to_string() != SEMANTIC_RUNTIME_PROGRAM.source();
        #[cfg(feature = "native-agentic-semantic-probe")]
        let source_mismatch = source.to_string() != program_probe::source();
        if Retained::as_ptr(&script) != Retained::as_ptr(&active.script)
            || source_mismatch
            || injection_time != WKUserScriptInjectionTime::AtDocumentStart
            || !main_frame_only
        {
            return Err(());
        }
        let _ = &active.handler;
        Ok(())
    }

    pub(crate) fn retire(mut self) -> Result<(), ()> {
        let channel_clean = self.channel.retire();
        let removed = clear_semantic_runtime_controller(&self.controller, &self.handler_name, None);
        if let Ok(mut epochs) = self.epochs.try_borrow_mut() {
            epochs.active = None;
            epochs.active_runtime = None;
            epochs.parked.clear();
        }
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
        let _ = clear_semantic_runtime_controller(&self.controller, &self.handler_name, None);
        if let Ok(mut epochs) = self.epochs.try_borrow_mut() {
            epochs.active = None;
            epochs.active_runtime = None;
            epochs.parked.clear();
        }
        self.retired = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use zephium_agentic::{
        encode_semantic_runtime_invocation, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameId, SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticRuntimeBudget, SemanticRuntimeFault, SemanticSnapshotGeneration,
    };

    fn pending_action(handed_off: bool) -> (SemanticRuntimeChannelState, SemanticActionAttemptId) {
        let native = crate::agent_context_port::WorkActionTask::native_for_test();
        let invocation = encode_semantic_action_runtime_invocation_for_test(&native);
        let attempt = invocation.attempt();
        let mut state = SemanticRuntimeChannelState {
            phase: DocumentPhase::Ready,
            expected_view: Some(7),
            active_world: Some(8),
            ..SemanticRuntimeChannelState::default()
        };
        state
            .dispatch_action(invocation, Box::new(|_| {}), Some(Box::new(|| true)))
            .unwrap_or_else(|_| panic!("fixture admission"));
        if handed_off {
            state.on_pull(reply());
        }
        (state, attempt)
    }

    fn encode_semantic_action_runtime_invocation_for_test(
        native: &zephium_agentic::SemanticActionNativeRequest,
    ) -> SemanticActionRuntimeInvocation {
        zephium_agentic::encode_semantic_action_runtime_invocation(native).unwrap()
    }

    #[cfg(feature = "native-agentic-semantic-probe")]
    #[test]
    fn url_drift_stops_existing_and_late_preparation_replies_but_keeps_original_terminal() {
        for preparation_first in [false, true] {
            let (mut state, attempt) = pending_action(true);
            if preparation_first {
                let actions = state.on_fill_preparation(reply());
                assert!(actions.first_reply.is_none());
                assert!(state.prepared_fill.is_some());
            }
            let actions = state.revoke_document_authority();
            assert_eq!(actions.first_reply.is_some(), preparation_first);
            assert!(actions.completion.is_none());
            assert!(state.prepared_fill.is_none());
            let late = state.on_fill_preparation(reply());
            assert!(matches!(late.first_reply.unwrap().value, ReplyValue::Error));
            assert!(late.completion.is_none());
            assert!(state.poll_fill_preparation().first_reply.is_none());
            assert!(state.draining_action(attempt));
            assert!(matches!(
                state
                    .on_result(b"E2:applied_unverified", reply())
                    .completion,
                Some(CompletionAction::Action {
                    outcome: Err(AgentSemanticActionRuntimeFailure::Result(_)),
                    ..
                })
            ));
        }
    }

    #[test]
    fn url_drift_revokes_admission_but_drains_only_the_original_handed_off_action() {
        for handed_off in [false, true] {
            let (mut state, attempt) = pending_action(handed_off);
            let actions = state.revoke_document_authority();
            assert_eq!(state.phase, DocumentPhase::AuthorityRevoked);
            assert_eq!(state.draining_action(attempt), handed_off);
            assert_eq!(actions.completion.is_some(), !handed_off);
            assert_eq!(
                state.admission_failure(),
                Some(AgentSemanticRuntimeDispatchError::Retired)
            );
            assert_eq!(
                &*success_value(state.on_pull(reply()).first_reply.unwrap()),
                SEMANTIC_RUNTIME_CHANNEL_STOP
            );
            if handed_off {
                assert!(state.revoke_document_authority().completion.is_none());
                let mut actions = state.on_result(b"E2:applied_unverified", reply());
                assert!(matches!(
                    actions.completion.take(),
                    Some(CompletionAction::Action {
                        outcome: Err(AgentSemanticActionRuntimeFailure::Result(
                            SemanticActionRuntimeResultError::Runtime(
                                zephium_agentic::SemanticActionRuntimeFault::AppliedUnverified
                            )
                        )),
                        ..
                    })
                ));
                assert_eq!(
                    &*success_value(actions.first_reply.unwrap()),
                    SEMANTIC_RUNTIME_CHANNEL_STOP
                );
                assert!(!state.draining_action(attempt));
                assert_eq!(state.phase, DocumentPhase::AuthorityRevoked);
                assert!(state.pending.is_none());
                assert_eq!(
                    state.admission_failure(),
                    Some(AgentSemanticRuntimeDispatchError::Retired)
                );
            }
        }
    }

    #[test]
    fn uncertain_terminal_retains_only_exact_passive_lifetime_across_url_drift() {
        for terminal in [
            b"E2:applied_unverified".as_slice(),
            b"E2:applied_unverified_logical_editor",
            b"E2:applied_unverified_postcondition",
        ] {
            for drift_first in [false, true] {
                let (mut state, attempt) = pending_action(true);
                if drift_first {
                    state.revoke_document_authority();
                    assert!(!state.settling_action(attempt));
                }
                let result = state.on_result(terminal, reply());
                assert!(result.completion.is_some());
                assert!(state.settling_action(attempt));
                assert!(!state
                    .settling_action(SemanticActionAttemptId::new(attempt.get() + 1).unwrap()));
                for _ in 0..2 {
                    let revoked = state.revoke_document_authority();
                    assert!(revoked.completion.is_none());
                    assert!(state.revoked_settling_action(attempt));
                    assert!(!state.draining_action(attempt));
                    assert_eq!(
                        state.admission_failure(),
                        Some(AgentSemanticRuntimeDispatchError::Retired)
                    );
                    assert_eq!(
                        &*success_value(state.on_pull(reply()).first_reply.unwrap()),
                        SEMANTIC_RUNTIME_CHANNEL_STOP
                    );
                }
            }
        }
    }

    #[test]
    fn passive_lifetime_dies_with_document_controls_or_malformed_terminal() {
        for edge in 0..5 {
            let (mut state, attempt) = pending_action(true);
            state.on_result(b"E2:applied_unverified", reply());
            state.revoke_document_authority();
            assert!(state.revoked_settling_action(attempt));
            let result = match edge {
                0 => state.cancel(),
                1 => state.begin_document_load(),
                2 => state.renderer_lost(),
                3 => state.retire(),
                _ => state.on_result(b"E2:applied_unverified", reply()),
            };
            assert!(
                result.completion.is_none(),
                "terminal must never be delivered twice"
            );
            assert!(!state.settling_action(attempt));
            assert!(!state.revoked_settling_action(attempt));
            assert!(state.settling_action.is_none());
        }
        let (mut state, attempt) = pending_action(true);
        state.revoke_document_authority();
        state.on_result(b"malformed", reply());
        assert!(!state.settling_action(attempt));
    }

    #[test]
    fn revoked_action_missing_terminal_times_out_exactly_and_stronger_edges_consume_it_once() {
        for edge in 0..5 {
            let (mut state, attempt) = pending_action(true);
            state.revoke_document_authority();
            let (unmatched, matched) =
                state.timeout_action(SemanticActionAttemptId::new(attempt.get() + 1).unwrap());
            assert!(!matched);
            assert!(unmatched.completion.is_none());
            assert!(state.draining_action(attempt));
            let actions = match edge {
                0 => {
                    let (actions, matched) = state.timeout_action(attempt);
                    assert!(matched);
                    actions
                }
                1 => state.cancel(),
                2 => state.begin_document_load(),
                3 => state.renderer_lost(),
                _ => state.retire(),
            };
            let Some(CompletionAction::Action {
                outcome: Err(failure),
                ..
            }) = actions.completion
            else {
                panic!("original failure");
            };
            assert!(matches!(
                (edge, failure),
                (0, AgentSemanticActionRuntimeFailure::TimedOut)
                    | (1, AgentSemanticActionRuntimeFailure::Cancelled)
                    | (2, AgentSemanticActionRuntimeFailure::DocumentReplaced)
                    | (3, AgentSemanticActionRuntimeFailure::RendererLost)
                    | (4, AgentSemanticActionRuntimeFailure::Retired)
            ));
            assert!(!state.draining_action(attempt));
            assert!(state
                .on_result(b"E2:applied_unverified", reply())
                .completion
                .is_none());
        }
    }

    #[test]
    fn url_drift_consumes_inflight_observation_and_never_reopens_on_action_success() {
        let (mut state, _) = pending_action(true);
        let Some(PendingInvocation::Action {
            invocation: action, ..
        }) = &state.pending
        else {
            panic!("action");
        };
        let wire = serde_json::json!({"v":1,"a":action.attempt().get(),"i":action.checkpoint_invocation().get(),"g":action.checkpoint_snapshot().get(),"r":"form","x":10,"y":20,"w":80,"h":30,"vw":800,"vh":600,"px":50,"py":35,"d":0,"b":"fixed_semantic_recipe"}).to_string();
        state.revoke_document_authority();
        let actions = state.on_result(wire.as_bytes(), reply());
        assert!(matches!(
            actions.completion,
            Some(CompletionAction::Action { outcome: Ok(_), .. })
        ));
        assert_eq!(state.phase, DocumentPhase::AuthorityRevoked);
        assert_eq!(
            state.admission_failure(),
            Some(AgentSemanticRuntimeDispatchError::Retired)
        );

        let mut state = SemanticRuntimeChannelState {
            phase: DocumentPhase::Ready,
            expected_view: Some(7),
            active_world: Some(8),
            ..SemanticRuntimeChannelState::default()
        };
        state
            .dispatch_observation(
                invocation(1, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            )
            .unwrap_or_else(|_| panic!("observation"));
        state.on_pull(reply());
        assert!(matches!(
            state.revoke_document_authority().completion,
            Some(CompletionAction::Observation {
                outcome: Err(AgentSemanticRuntimeFailure::DocumentReplaced),
                ..
            })
        ));
        assert!(state.pending.is_none());
    }

    #[test]
    fn retained_action_rechecks_authority_at_page_pull_before_releasing_recipe() {
        for revoked in [false, true] {
            let native = crate::agent_context_port::WorkActionTask::native_for_test();
            let invocation =
                zephium_agentic::encode_semantic_action_runtime_invocation(&native).unwrap();
            let encoded = invocation.as_str().to_owned();
            let allowed = Rc::new(std::cell::Cell::new(true));
            let fence = allowed.clone();
            let mut state = SemanticRuntimeChannelState {
                phase: DocumentPhase::Ready,
                expected_view: Some(7),
                active_world: Some(8),
                ..SemanticRuntimeChannelState::default()
            };
            let pending = state
                .dispatch_action(
                    invocation,
                    Box::new(|_| {}),
                    Some(Box::new(move || fence.get())),
                )
                .unwrap_or_else(|_| panic!("fixture admission"));
            assert!(pending.first_reply.is_none());
            assert!(pending.completion.is_none());
            allowed.set(!revoked);
            let mut actions = state.on_message(SEMANTIC_RUNTIME_CHANNEL_PULL, reply());
            let delivered = success_value(actions.first_reply.take().unwrap());
            if revoked {
                assert_eq!(&*delivered, SEMANTIC_RUNTIME_CHANNEL_STOP);
                assert!(matches!(
                    actions.completion,
                    Some(CompletionAction::Action {
                        outcome: Err(AgentSemanticActionRuntimeFailure::Cancelled),
                        ..
                    })
                ));
                assert!(state.pending.is_none());
                assert!(!state.awaiting_result);
                assert_eq!(state.phase, DocumentPhase::Failed);
            } else {
                assert_eq!(&*delivered, encoded);
                assert!(actions.completion.is_none());
                assert!(state.awaiting_result);
            }
        }
    }

    #[test]
    fn owned_view_fill_is_private_isolated_recipe_without_page_entry_point() {
        let source = SEMANTIC_RUNTIME_PROGRAM.source();
        assert!(source.contains("function runFixedFill(target, descriptor, request)"));
        assert!(source.contains("resolveKeyAtGeneration(request.t, request.g) !== target"));
        assert!(
            source.contains("descriptorMatches(request.f, runtimeDescriptor(target, request.g))")
        );
        assert!(source.contains("const fixedDispatchEvent = EventTarget.prototype.dispatchEvent"));
        for forbidden in ["CustomEvent", "PAGE_RELAY", "querySelector", "eval("] {
            assert!(
                !source.contains(forbidden),
                "forbidden isolated fill surface: {forbidden}"
            );
        }
        assert_eq!(
            source
                .matches("EventTarget.prototype.addEventListener")
                .count(),
            1
        );
        assert!(!source.contains(".addEventListener("));
    }
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
        assert!(matches!(
            state.dispatch_observation(
                invocation(10, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            ),
            Err((AgentSemanticRuntimeDispatchError::NotReady, _))
        ));
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
            .dispatch_observation(invocation, Box::new(|_| {}))
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
            actions
                .completion
                .expect("completion")
                .observation_outcome(),
            Err(AgentSemanticRuntimeFailure::Result(
                SemanticRuntimeResultError::Runtime(SemanticRuntimeFault::Busy)
            ))
        ));
        assert_eq!(state.completed_invocations, 1);
        assert!(!state.awaiting_result);
        assert!(state.pending.is_none());
    }

    #[test]
    fn park_requires_idle_pull_and_acknowledges_before_reactivation() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(7),
            active_world: Some(8),
            phase: DocumentPhase::Ready,
            pull: Some(reply()),
            completed_invocations: 19,
            ..SemanticRuntimeChannelState::default()
        };
        let actions = state
            .begin_park(Box::new(|_| {}))
            .unwrap_or_else(|_| panic!("park"));
        assert_eq!(
            success_value(actions.first_reply.expect("park request")).as_ref(),
            SEMANTIC_RUNTIME_CHANNEL_PARK
        );
        assert_eq!(state.phase, DocumentPhase::Parking);
        assert!(state.pull.is_none());
        assert!(state.park_completion.is_some());

        let mut actions = state.on_message(SEMANTIC_RUNTIME_CHANNEL_PARKED, reply());
        assert_eq!(
            success_value(actions.first_reply.take().expect("park ack")).as_ref(),
            SEMANTIC_RUNTIME_CHANNEL_ACK
        );
        assert!(matches!(actions.park_completion.take(), Some((_, true))));
        assert_eq!(state.phase, DocumentPhase::Parked);
        assert_eq!(state.completed_invocations, 0);
        assert!(state.pending.is_none());
        assert!(!state.awaiting_result);
        assert!(state.pull.is_none());

        state.reactivate(9).expect("reactivate");
        assert_eq!(state.phase, DocumentPhase::Loading);
        let actions = state.on_message(SEMANTIC_RUNTIME_CHANNEL_PULL, reply());
        assert!(actions.first_reply.is_none());
        assert!(state.pull.is_some());
        let actions = state.document_committed();
        assert!(!actions.invariant_failed);
        assert_eq!(state.phase, DocumentPhase::Ready);
    }

    #[test]
    fn park_refuses_busy_state_and_lifecycle_loss_completes_failure_once() {
        let mut busy = SemanticRuntimeChannelState {
            expected_view: Some(7),
            active_world: Some(8),
            phase: DocumentPhase::Ready,
            pull: Some(reply()),
            pending: Some(PendingInvocation::Observation {
                invocation: invocation(1, SemanticSnapshotGeneration::INITIAL),
                completion: Box::new(|_| {}),
            }),
            ..SemanticRuntimeChannelState::default()
        };
        assert!(busy.begin_park(Box::new(|_| {})).is_err());

        busy.pending = None;
        let _ = busy
            .begin_park(Box::new(|_| {}))
            .unwrap_or_else(|_| panic!("park"));
        let mut actions = busy.renderer_lost();
        assert!(matches!(actions.park_completion.take(), Some((_, false))));
        assert!(busy.park_completion.is_none());
        assert_eq!(busy.phase, DocumentPhase::RendererLost);
        let actions = busy.renderer_lost();
        assert!(actions.park_completion.is_none());
    }

    #[test]
    fn failed_park_reply_delivers_one_false_completion() {
        let calls = Rc::new(Cell::new(0_u8));
        let value = Rc::new(Cell::new(true));
        let callback_calls = calls.clone();
        let callback_value = value.clone();
        let primary: SemanticParkCompletion = Box::new(move |parked| {
            callback_calls.set(callback_calls.get() + 1);
            callback_value.set(parked);
        });
        let (completion, duplicate) =
            reconcile_failed_reply_park_completion(Some((primary, true)), None);
        assert!(!duplicate);
        let (completion, parked) = completion.expect("completion");
        completion(parked);
        assert_eq!(calls.get(), 1);
        assert!(!value.get());
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
            state.dispatch_observation(
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
            .dispatch_observation(invocation, Box::new(|_| {}))
            .unwrap_or_else(|_| panic!("dispatch"))
            .first_reply
            .is_none());
        let actions = state.begin_document_load();
        assert!(matches!(
            actions
                .completion
                .expect("replacement")
                .observation_outcome(),
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
            state.dispatch_observation(
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
            .dispatch_observation(
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
            actions
                .completion
                .expect("timeout completion")
                .observation_outcome(),
            Err(AgentSemanticRuntimeFailure::TimedOut)
        ));
        assert_eq!(state.phase, DocumentPhase::Failed);
        assert!(state.pending.is_none());
        assert!(state
            .dispatch_observation(
                invocation(23, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            )
            .is_err());
    }
}
