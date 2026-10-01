//! Lease ownership for already policy-authorized semantic actions. This module
//! does not classify effects, approve actions, or certify their observed result.

use super::*;
use crate::semantic_execute::SemanticActionNativeCorrelation;
use crate::{SemanticActionNativeRequest, SemanticActionNativeSettlement, SemanticFrameJoin};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ActionJoin {
    lease: WorkBrowserExecutionLease,
    frame: SemanticFrameJoin,
    correlation: SemanticActionNativeCorrelation,
    authority: Authority,
}

/// An unsuccessful transition preserves its original move-only owners. A
/// refusal never manufactures a native terminal or releases another operation.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserActionRefusal<T> {
    error: WorkBrowserResourceError,
    original: T,
}
impl<T> WorkBrowserActionRefusal<T> {
    fn new(error: WorkBrowserResourceError, original: T) -> Box<Self> {
        Box::new(Self { error, original })
    }
    /// Content-free reason; the original request or receipt remains owned.
    pub const fn error(&self) -> WorkBrowserResourceError {
        self.error
    }
    /// Recover all original operands for policy refusal or explicit recovery.
    pub fn into_parts(self) -> (WorkBrowserResourceError, T) {
        (self.error, self.original)
    }
}

/// Exactly one existing policy-authorized recipe, bound to the original
/// retained lease and latest observed document checkpoint. It is not cloneable.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserActionRequest {
    join: ActionJoin,
    native: SemanticActionNativeRequest,
    delivery: super::delivery::DeliveryDispatch,
    ticket: Option<WorkBrowserActionDeliveryTicket>,
}
impl WorkBrowserActionRequest {
    /// App-owned exact callback-return barrier, registered before dispatch.
    pub fn take_delivery_ticket(&mut self) -> Option<WorkBrowserActionDeliveryTicket> {
        self.ticket.take()
    }
    /// Exact resource incarnation and run lease, not account/effect authority.
    pub const fn lease(&self) -> &WorkBrowserExecutionLease {
        &self.join.lease
    }
    /// Existing closed native recipe. The adapter must revalidate it at the
    /// point of effect under the same lease, document and native deadline.
    pub const fn action(&self) -> &SemanticActionNativeRequest {
        &self.native
    }
    /// Transfers the recipe to the existing native executor and retains a
    /// separate exact-terminal owner. Neither half may be replaced on timeout.
    pub fn into_parts(
        self,
    ) -> (
        SemanticActionNativeRequest,
        WorkBrowserActionCompletionOwner,
    ) {
        (
            self.native,
            WorkBrowserActionCompletionOwner {
                join: self.join,
                delivery: self
                    .delivery
                    .completion
                    .map(WorkBrowserActionDeliveryCompletion),
            },
        )
    }
}

/// Original terminal owner retained while the native recipe executes. Dropping
/// this owner leaves resource debt; it cannot imply cancellation or native drain.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserActionCompletionOwner {
    join: ActionJoin,
    delivery: Option<WorkBrowserActionDeliveryCompletion>,
}
impl WorkBrowserActionCompletionOwner {
    /// Native must retain this until the original callback returned and its
    /// action guard permits a fresh observation. Dropping it proves no return.
    pub fn take_delivery_completion(&mut self) -> Option<WorkBrowserActionDeliveryCompletion> {
        self.delivery.take()
    }
    /// Joins only the original native terminal, including native failure. A
    /// substituted terminal returns both operands without clearing resource debt.
    pub fn settle(
        self,
        terminal: SemanticActionNativeSettlement,
    ) -> Result<
        WorkBrowserActionCompletion,
        Box<WorkBrowserActionRefusal<(Self, SemanticActionNativeSettlement)>>,
    > {
        if !self.join.correlation.matches(&terminal) {
            return Err(WorkBrowserActionRefusal::new(
                WorkBrowserResourceError::Stale,
                (self, terminal),
            ));
        }
        Ok(WorkBrowserActionCompletion {
            join: self.join,
            terminal,
        })
    }
}

/// Exact callback awaiting resource accounting. Its policy/effect owner remains
/// independent and must receive the same native terminal after this transition.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserActionCompletion {
    join: ActionJoin,
    terminal: SemanticActionNativeSettlement,
}

/// Resource-accounted terminal. `is_current` is lease health only; success still
/// requires native-outcome admission, a fresh observation and policy settlement.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserActionEvent {
    terminal: SemanticActionNativeSettlement,
    current: bool,
}
impl WorkBrowserActionEvent {
    /// True only while the same lease/document remains available. No action
    /// outcome, fresh references or permission to continue is implied.
    pub const fn is_current(&self) -> bool {
        self.current
    }
    /// Return the original native terminal to its independent policy owner,
    /// even after expiry, revocation, quarantine or destruction.
    pub fn into_terminal(self) -> SemanticActionNativeSettlement {
        self.terminal
    }
}

impl WorkBrowserResources {
    /// Reserve one already-authorized action against this original resource.
    /// The native request can only be created by the existing effect authority;
    /// page labels, tool arguments and a lease alone cannot create this request.
    ///
    /// Serializes with reads/navigation and retires current observation authority
    /// before any dispatch. Failure preserves the native request so the caller
    /// can account synchronous non-admission with its original policy owner.
    pub fn prepare_action(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        native: SemanticActionNativeRequest,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserActionRequest, Box<WorkBrowserActionRefusal<SemanticActionNativeRequest>>>
    {
        let checked = (|| {
            let binding = self.read_binding(lease, now)?;
            let row = self.row_mut(lease.resource())?;
            if row.observation.is_some() {
                return Err(WorkBrowserResourceError::Pending);
            }
            if !row.observed
                || native.frame() != binding.frame()
                || native.checkpoint_invocation().get() != u64::from(row.observation_sequence)
                || native.checkpoint_snapshot().get() != u64::from(row.observation_sequence)
            {
                return Err(WorkBrowserResourceError::Stale);
            }
            if native.requested_at().millis() > now.millis()
                || native.deadline().millis() > lease.deadline().millis()
                || now.millis() >= native.deadline().millis()
            {
                return Err(WorkBrowserResourceError::Expired);
            }
            let join = ActionJoin {
                lease: lease.clone(),
                frame: binding.frame().clone(),
                correlation: native.correlation(),
                authority: Authority(Arc::new(())),
            };
            row.observed = false;
            row.action = Some(join.clone());
            Ok(join)
        })();
        match checked {
            Ok(join) => {
                let (delivery, ticket) = super::delivery::track(lease.clone());
                Ok(WorkBrowserActionRequest {
                    join,
                    native,
                    delivery,
                    ticket: Some(WorkBrowserActionDeliveryTicket(ticket)),
                })
            }
            Err(error) => Err(WorkBrowserActionRefusal::new(error, native)),
        }
    }

    /// Exact synchronous non-admission. No callback or native success is
    /// synthesized. Old observations stay retired; subsequent work must observe
    /// again after independent policy refusal. A stale request stays owned.
    pub fn action_dispatch_refused(
        &mut self,
        request: WorkBrowserActionRequest,
    ) -> Result<SemanticActionNativeRequest, Box<WorkBrowserActionRefusal<WorkBrowserActionRequest>>>
    {
        let result = (|| {
            let row = self.row_mut(request.join.lease.resource())?;
            if row.action.as_ref() != Some(&request.join) {
                return Err(WorkBrowserResourceError::Stale);
            }
            row.action = None;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(request.native),
            Err(error) => Err(WorkBrowserActionRefusal::new(error, request)),
        }
    }

    /// Account the original callback without discarding native evidence when
    /// execution authority expires. The core never fabricates a terminal from
    /// a timeout or wake. Fresh observation and independent policy verification
    /// are mandatory before an action can be reported as verified.
    pub fn settle_action(
        &mut self,
        completion: WorkBrowserActionCompletion,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserActionEvent, Box<WorkBrowserActionRefusal<WorkBrowserActionCompletion>>>
    {
        let sealed = self.sealed;
        let result = (|| {
            let row = self.row_mut(completion.join.lease.resource())?;
            if row.action.as_ref() != Some(&completion.join) {
                return Err(WorkBrowserResourceError::Stale);
            }
            let prior = row.phase;
            let _ = row.tick(now);
            if matches!(
                prior,
                WorkBrowserResourcePhase::Destroying | WorkBrowserResourcePhase::Destroyed
            ) {
                row.phase = prior;
            }
            row.action = None;
            Ok(!sealed
                && row.failure.is_none()
                && row.phase == WorkBrowserResourcePhase::Leased
                && row.lease.as_ref() == Some(&completion.join.lease)
                && now < completion.join.lease.deadline
                && row.document_available
                && row.navigation_epoch == completion.join.frame.context().navigation_epoch()
                && row.frame_generation == completion.join.frame.context().frame_generation())
        })();
        match result {
            Ok(current) => Ok(WorkBrowserActionEvent {
                terminal: completion.terminal,
                current,
            }),
            Err(error) => Err(WorkBrowserActionRefusal::new(error, completion)),
        }
    }
}

/// Exact action callback-return ticket. Deliberately cannot produce a lease
/// retirement proof or grant effect authority.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserActionDeliveryTicket(super::WorkBrowserLeaseDeliveryTicket);
impl WorkBrowserActionDeliveryTicket {
    /// Register the sole app notification listener before native dispatch.
    pub fn register_waker(
        &mut self,
        waker: std::task::Waker,
    ) -> Result<(), super::WorkBrowserLeaseDeliveryPollError> {
        self.0.register_waker(waker)
    }
    /// Consume only the original physical-return fact. False is unproven.
    pub fn try_take_returned(
        &mut self,
    ) -> Result<Option<bool>, super::WorkBrowserLeaseDeliveryPollError> {
        self.0
            .try_take()
            .map(|receipt| receipt.map(|receipt| receipt.returned()))
    }
}
/// Native action callback-return publisher, unrelated to lease retirement.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserActionDeliveryCompletion(super::WorkBrowserLeaseDeliveryCompletion);
impl WorkBrowserActionDeliveryCompletion {
    /// Take before terminal callback; invoke outside native locks after publish.
    pub fn take_notification(&mut self) -> Option<super::WorkBrowserLeaseDeliveryNotification> {
        self.0.take_notification()
    }
    /// Call only after the exact callback returned normally and action/native
    /// task debt no longer blocks the successor observation.
    pub fn publish_returned(self) -> bool {
        self.0.publish_returned()
    }
}

/// Native owns one exact callback after successful dispatch. The app/engine
/// must additionally prove its physical return before lease retirement.
pub type WorkBrowserActionCompletionCallback =
    Box<dyn FnOnce(WorkBrowserActionCompletion) + Send + 'static>;

/// Lossless native admission; no adapter is implicitly enabled by this type.
#[must_use]
#[derive(Debug)]
pub enum WorkBrowserActionDispatch {
    /// Native owns the original request and completion obligation.
    Scheduled,
    /// No action/callback transferred. Account the exact original request in
    /// the resource rows and independently refuse the policy reservation.
    Rejected {
        /// Original unmodified request.
        request: Box<WorkBrowserActionRequest>,
        /// Content-free adapter refusal.
        failure: ContextPortFailure,
    },
}

#[cfg(test)]
#[path = "work_browser_action_tests.rs"]
mod tests;
