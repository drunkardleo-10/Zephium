//! Closed Store-actor boundary for durable agent audit delivery.

#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock, TryLockError};

use zephium_agentic::{
    AgentAuditCompletion, AgentAuditDelivery, AgentAuditDeliveryOutcome, AgentAuditDeliveryProof,
    AgentAuditDeliverySettlement, AgentAuditDispatch, AgentAuditPort, AgentAuditSinkFailure,
};

use super::{Cmd, SqliteStore};
use crate::hub::{self, Hub};

/// Independent ceiling below the general Store mailbox capacity. The lazy
/// counter leaves no allocation or synchronization object when agents are not
/// used.
pub(super) const MAX_PENDING_AGENT_AUDIT_DELIVERIES: usize = 8;

pub(super) struct AgentAuditDeliveryPermit {
    admission: Arc<AtomicUsize>,
}

impl AgentAuditDeliveryPermit {
    fn acquire(admission: &OnceLock<Arc<AtomicUsize>>) -> Option<Self> {
        let admission = admission
            .get_or_init(|| Arc::new(AtomicUsize::new(0)))
            .clone();
        admission
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending
                    .checked_add(1)
                    .filter(|next| *next <= MAX_PENDING_AGENT_AUDIT_DELIVERIES)
            })
            .ok()?;
        Some(Self { admission })
    }
}

impl Drop for AgentAuditDeliveryPermit {
    fn drop(&mut self) {
        if self
            .admission
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending.checked_sub(1)
            })
            .is_err()
        {
            // Accounting drift permanently poisons this independent admission
            // counter instead of reopening an effectively unbounded queue.
            self.admission.store(usize::MAX, Ordering::Release);
        }
    }
}

impl AgentAuditPort for SqliteStore {
    fn append(
        &self,
        delivery: AgentAuditDelivery,
        completion: AgentAuditCompletion,
    ) -> AgentAuditDispatch {
        let proof = delivery.proof();
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                return refuse_without_callback(
                    proof,
                    AgentAuditSinkFailure::Unavailable,
                    completion,
                );
            }
        };
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return refuse_without_callback(proof, AgentAuditSinkFailure::Shutdown, completion);
        }
        let Some(permit) = AgentAuditDeliveryPermit::acquire(&self.agent_audit_delivery_admission)
        else {
            return refuse_without_callback(proof, AgentAuditSinkFailure::Capacity, completion);
        };
        let dispatch = match self
            .tx
            .try_send(Cmd::AppendAgentAudit(delivery, permit, completion))
        {
            Ok(()) => AgentAuditDispatch::Accepted(proof),
            Err(error) => refuse_queued(error, proof),
        };
        drop(lifecycle);
        dispatch
    }
}

#[derive(Clone, Copy)]
pub(super) enum AgentAuditActorResult {
    Settled,
    Uncertain,
    CompletionPanicked,
}

impl AgentAuditActorResult {
    /// The general Store actor owns diagnostics. Expose only fixed,
    /// content-free literals so neither a delivery nor a SQLite error can
    /// cross that boundary.
    pub(super) const fn diagnostic(self) -> Option<&'static str> {
        match self {
            Self::Settled => None,
            Self::Uncertain => Some("store: agent audit append outcome is uncertain"),
            Self::CompletionPanicked => Some("store: agent audit completion callback panicked"),
        }
    }
}

pub(super) fn append_and_settle(
    hub: &mut Hub,
    delivery: AgentAuditDelivery,
    completion: AgentAuditCompletion,
) -> AgentAuditActorResult {
    let proof = delivery.proof();
    let settlement = match hub.append_agent_audit(&delivery) {
        hub::AgentAuditAppendOutcome::Committed => {
            proof.settle(AgentAuditDeliveryOutcome::Committed)
        }
        hub::AgentAuditAppendOutcome::Refused(failure) => {
            proof.settle(AgentAuditDeliveryOutcome::Refused(failure))
        }
        hub::AgentAuditAppendOutcome::Uncertain => {
            // Dropping the one-shot callback is intentional. Its caller
            // retains the ledger's exact in-flight delivery and may replay
            // only that identity for reconciliation.
            discard_callback(completion);
            return AgentAuditActorResult::Uncertain;
        }
    };
    if complete(completion, settlement) {
        AgentAuditActorResult::Settled
    } else {
        AgentAuditActorResult::CompletionPanicked
    }
}

fn complete(completion: AgentAuditCompletion, settlement: AgentAuditDeliverySettlement) -> bool {
    // The durable result is already final. Contain an integration callback
    // panic so it cannot kill the Store actor or strand unrelated state.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(settlement))).is_ok()
}

fn refuse_without_callback(
    proof: AgentAuditDeliveryProof,
    failure: AgentAuditSinkFailure,
    completion: AgentAuditCompletion,
) -> AgentAuditDispatch {
    let dispatch =
        AgentAuditDispatch::Refused(proof.settle(AgentAuditDeliveryOutcome::Refused(failure)));
    discard_callback(completion);
    dispatch
}

fn refuse_queued(
    error: mpsc::TrySendError<Cmd>,
    proof: AgentAuditDeliveryProof,
) -> AgentAuditDispatch {
    let failure = match &error {
        mpsc::TrySendError::Full(_) => AgentAuditSinkFailure::Capacity,
        mpsc::TrySendError::Disconnected(_) => AgentAuditSinkFailure::Shutdown,
    };
    let command = match error {
        mpsc::TrySendError::Full(command) | mpsc::TrySendError::Disconnected(command) => command,
    };
    if let Cmd::AppendAgentAudit(_delivery, _permit, completion) = command {
        discard_callback(completion);
    }
    AgentAuditDispatch::Refused(proof.settle(AgentAuditDeliveryOutcome::Refused(failure)))
}

fn discard_callback(completion: AgentAuditCompletion) {
    // A caller-owned closure may carry a panicking destructor even when the
    // callback must not be invoked. Contain that foreign code on every
    // pre-admission, mailbox-refusal, and commit-ambiguous discard path.
    let _discard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(completion)));
}
