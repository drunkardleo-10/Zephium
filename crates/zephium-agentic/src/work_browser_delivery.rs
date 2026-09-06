//! One exact revocation-delivery rendezvous. Polling consumes a native fact;
//! it does not dispatch work or synchronously wait inside a terminal callback.

use super::*;
use std::sync::atomic::{AtomicU8, Ordering};

const PENDING: u8 = 0;
const RETURNED: u8 = 1;
const UNPROVEN: u8 = 2;
const CONSUMED: u8 = 3;
const ABANDONED: u8 = 4;

#[derive(Clone)]
pub(super) struct DeliveryBinding(Arc<AtomicU8>);

impl DeliveryBinding {
    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for DeliveryBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeliveryBinding([redacted])")
    }
}

#[derive(Debug)]
pub(super) struct DeliveryDispatch {
    pub(super) binding: DeliveryBinding,
    pub(super) completion: Option<WorkBrowserLeaseDeliveryCompletion>,
}

pub(super) fn track(
    lease: WorkBrowserExecutionLease,
) -> (DeliveryDispatch, WorkBrowserLeaseDeliveryTicket) {
    let binding = DeliveryBinding(Arc::new(AtomicU8::new(PENDING)));
    (
        DeliveryDispatch {
            binding: binding.clone(),
            completion: Some(WorkBrowserLeaseDeliveryCompletion {
                binding: binding.clone(),
            }),
        },
        WorkBrowserLeaseDeliveryTicket { lease, binding },
    )
}

/// One request-bound, non-cloneable slot for native delivery completion.
/// No callback, timer, worker, native object or execution authority is retained.
/// The owning application must arrange bounded, wake-driven polling; never spin
/// or synchronously wait for this slot from the revocation callback itself.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserLeaseDeliveryTicket {
    lease: WorkBrowserExecutionLease,
    binding: DeliveryBinding,
}

impl WorkBrowserLeaseDeliveryTicket {
    /// Takes the exact terminal once. `None` means delivery is still pending,
    /// not that no native debt exists. An unhandled completion yields an
    /// explicitly unproven receipt, never an implicit returned callback.
    pub fn try_take(
        &mut self,
    ) -> Result<Option<WorkBrowserLeaseDeliveryReceipt>, WorkBrowserLeaseDeliveryPollError> {
        let state = self.binding.0.load(Ordering::Acquire);
        match state {
            PENDING => Ok(None),
            RETURNED | UNPROVEN => {
                self.binding
                    .0
                    .compare_exchange(state, CONSUMED, Ordering::AcqRel, Ordering::Acquire)
                    .map_err(|_| WorkBrowserLeaseDeliveryPollError::Consumed)?;
                Ok(Some(WorkBrowserLeaseDeliveryReceipt {
                    lease: self.lease.clone(),
                    binding: self.binding.clone(),
                    returned: state == RETURNED,
                }))
            }
            _ => Err(WorkBrowserLeaseDeliveryPollError::Consumed),
        }
    }
}

impl Drop for WorkBrowserLeaseDeliveryTicket {
    fn drop(&mut self) {
        let _ = self
            .binding
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state != CONSUMED && state != ABANDONED).then_some(ABANDONED)
            });
    }
}

/// Polling cannot replay an already-consumed exact delivery terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum WorkBrowserLeaseDeliveryPollError {
    /// This ticket's one terminal has already been consumed.
    #[error("Work lease delivery terminal was already consumed")]
    Consumed,
}

/// Move-only completion half retained by the original native lifecycle task.
/// Only the trusted native adapter may publish it, after its original callback
/// returned normally, its exact task permit released, and the same resource
/// passed its healthy-retained recheck. Merely receiving `LeaseEnded` is not
/// sufficient. An adapter that does not implement this barrier must drop it.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserLeaseDeliveryCompletion {
    binding: DeliveryBinding,
}

impl WorkBrowserLeaseDeliveryCompletion {
    /// Publishes that exact native delivery fact without invoking a callback.
    /// Returns false if the owning consumer was lost; native successor admission
    /// must then remain closed. This is a native attestation, not a global audit,
    /// current resource-health query, run outcome or worker-join proof.
    pub fn publish_returned(self) -> bool {
        self.binding
            .0
            .compare_exchange(PENDING, RETURNED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

impl Drop for WorkBrowserLeaseDeliveryCompletion {
    fn drop(&mut self) {
        let _ =
            self.binding
                .0
                .compare_exchange(PENDING, UNPROVEN, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// One consumed exact native-delivery terminal. It has no execution capability.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserLeaseDeliveryReceipt {
    lease: WorkBrowserExecutionLease,
    binding: DeliveryBinding,
    returned: bool,
}

impl WorkBrowserLeaseDeliveryReceipt {
    /// Whether the native adapter proved normal callback return and exact task
    /// release. Even true requires the matching core lease-ended receipt.
    pub const fn returned(&self) -> bool {
        self.returned
    }
}

/// Exact native lease retirement plus physical revocation-delivery drain.
/// It is not task success, policy/provider/audit closure, worker drain, resource
/// destruction, or global native shutdown. It cannot convert to those proofs.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserLeaseDeliveryProof {
    lease: WorkBrowserExecutionLease,
}

impl WorkBrowserLeaseDeliveryProof {
    /// Exact ended lease; its page was retained at native delivery completion.
    /// A later actor still requires fresh product admission and resource health.
    pub const fn lease(&self) -> &WorkBrowserExecutionLease {
        &self.lease
    }
}

/// Lossless refusal; a failed proof join cannot discard either original owner.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserLeaseDeliveryRefusal {
    ended: WorkBrowserLeaseEnded,
    receipt: WorkBrowserLeaseDeliveryReceipt,
}

impl WorkBrowserLeaseDeliveryRefusal {
    /// Returns both original, non-executing owners for exact reconciliation.
    pub fn into_parts(self) -> (WorkBrowserLeaseEnded, WorkBrowserLeaseDeliveryReceipt) {
        (self.ended, self.receipt)
    }
}

impl WorkBrowserLeaseEnded {
    /// Joins the matching consumed delivery receipt. A foreign request's slot,
    /// an untracked legacy terminal, or an unproven callback cannot qualify.
    pub fn join_delivery(
        self,
        receipt: WorkBrowserLeaseDeliveryReceipt,
    ) -> Result<WorkBrowserLeaseDeliveryProof, Box<WorkBrowserLeaseDeliveryRefusal>> {
        if receipt.returned
            && self.lease == receipt.lease
            && self
                .delivery
                .as_ref()
                .is_some_and(|binding| binding.matches(&receipt.binding))
        {
            Ok(WorkBrowserLeaseDeliveryProof { lease: self.lease })
        } else {
            Err(Box::new(WorkBrowserLeaseDeliveryRefusal {
                ended: self,
                receipt,
            }))
        }
    }
}
