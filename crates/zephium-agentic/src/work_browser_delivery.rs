//! One exact revocation-delivery rendezvous. Polling consumes a native fact;
//! it does not dispatch work or synchronously wait inside a terminal callback.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::task::Waker;

const PENDING: u8 = 0;
const RETURNED: u8 = 1;
const UNPROVEN: u8 = 2;
const CONSUMED: u8 = 3;
const ABANDONED: u8 = 4;
const LISTENER_REQUIRED: u8 = 1;
const NOTIFICATION_LOST: u8 = 2;

#[derive(Clone)]
pub(super) struct DeliveryBinding(Arc<DeliveryState>);

struct DeliveryState {
    state: AtomicU8,
    listener: DeliveryListener,
    notification_taken: AtomicBool,
}

struct DeliveryListener {
    waker: Mutex<Option<Arc<Waker>>>,
    // One RMW order joins registration and loss. Separate release/acquire
    // booleans could both miss the other side's concurrent publication.
    status: AtomicU8,
    signaled: AtomicBool,
    failed: AtomicBool,
}
impl DeliveryListener {
    fn notify(&self) -> bool {
        let waker = match self.waker.lock() {
            Ok(slot) => slot.clone(),
            Err(_) => {
                self.failed.store(true, Ordering::Release);
                return false;
            }
        };
        if let Some(waker) = waker {
            if !self.signaled.swap(true, Ordering::AcqRel)
                && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake_by_ref()))
                    .is_err()
            {
                self.failed.store(true, Ordering::Release);
            }
        }
        !self.failed.load(Ordering::Acquire)
    }
}

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
    let binding = DeliveryBinding(Arc::new(DeliveryState {
        state: AtomicU8::new(PENDING),
        notification_taken: AtomicBool::new(false),
        listener: DeliveryListener {
            waker: Mutex::new(None),
            status: AtomicU8::new(0),
            signaled: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        },
    }));
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
/// No terminal callback, timer, worker, native object or execution authority is retained.
/// The owning application must arrange bounded, wake-driven polling; never spin
/// or synchronously wait for this slot from the revocation callback itself.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserLeaseDeliveryTicket {
    lease: WorkBrowserExecutionLease,
    binding: DeliveryBinding,
}

impl WorkBrowserLeaseDeliveryTicket {
    /// Registers one immutable notification-only listener, then rechecks the
    /// already-published terminal. Call outside application/native locks: a
    /// ready slot may synchronously wake. Registration never consumes a receipt.
    pub fn register_waker(
        &mut self,
        waker: Waker,
    ) -> Result<(), WorkBrowserLeaseDeliveryPollError> {
        let waker = Arc::new(waker);
        let registered = match self.binding.0.listener.waker.lock() {
            Ok(mut slot) if slot.is_none() => {
                *slot = Some(waker);
                true
            }
            _ => false,
        };
        let lost = self
            .binding
            .0
            .listener
            .status
            .fetch_or(LISTENER_REQUIRED, Ordering::AcqRel)
            & NOTIFICATION_LOST
            != 0;
        if !registered || lost {
            self.binding
                .0
                .listener
                .failed
                .store(true, Ordering::Release);
        }
        if self.binding.0.state.load(Ordering::Acquire) != PENDING {
            self.binding.0.listener.notify();
        }
        if self.binding.0.listener.failed.load(Ordering::Acquire) {
            Err(WorkBrowserLeaseDeliveryPollError::Notification)
        } else {
            Ok(())
        }
    }
    /// Takes the exact terminal once. `None` means delivery is still pending,
    /// not that no native debt exists. An unhandled completion yields an
    /// explicitly unproven receipt, never an implicit returned callback.
    pub fn try_take(
        &mut self,
    ) -> Result<Option<WorkBrowserLeaseDeliveryReceipt>, WorkBrowserLeaseDeliveryPollError> {
        if self.binding.0.listener.waker.is_poisoned() {
            self.binding
                .0
                .listener
                .failed
                .store(true, Ordering::Release);
        }
        if self.binding.0.listener.failed.load(Ordering::Acquire) {
            return Err(WorkBrowserLeaseDeliveryPollError::Notification);
        }
        let state = self.binding.0.state.load(Ordering::Acquire);
        match state {
            PENDING => Ok(None),
            RETURNED | UNPROVEN => {
                self.binding
                    .0
                    .state
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
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state != CONSUMED && state != ABANDONED).then_some(ABANDONED)
            });
    }
}

/// Polling cannot replay an already-consumed exact delivery terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum WorkBrowserLeaseDeliveryPollError {
    /// An explicitly registered listener was lost, poisoned, replaced or failed.
    #[error("Work lease delivery notification could not be proven")]
    Notification,
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
    /// Takes the sole notification owner. Native code publishes the physical
    /// fact under its exact guard, then invokes this notification outside every
    /// native lock. Native successor ingress remains closed until that returns.
    pub fn take_notification(&mut self) -> Option<WorkBrowserLeaseDeliveryNotification> {
        if self
            .binding
            .0
            .notification_taken
            .swap(true, Ordering::AcqRel)
        {
            return None;
        }
        Some(WorkBrowserLeaseDeliveryNotification {
            binding: self.binding.clone(),
            notified: false,
        })
    }
    /// Publishes that exact native delivery fact without invoking a callback.
    /// Returns false if the owning consumer was lost; native successor admission
    /// must then remain closed. This is a native attestation, not a global audit,
    /// current resource-health query, run outcome or worker-join proof.
    pub fn publish_returned(self) -> bool {
        if self.binding.0.listener.status.load(Ordering::Acquire) & LISTENER_REQUIRED != 0
            && !self.binding.0.notification_taken.load(Ordering::Acquire)
        {
            self.binding
                .0
                .listener
                .failed
                .store(true, Ordering::Release);
            return false;
        }
        self.binding
            .0
            .state
            .compare_exchange(PENDING, RETURNED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

/// One notification-only handoff, not a terminal callback or delivery receipt.
/// The physical returned fact is immutable; notification failures independently
/// close admission and never manufacture, replace or erase that native fact.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserLeaseDeliveryNotification {
    binding: DeliveryBinding,
    notified: bool,
}
impl WorkBrowserLeaseDeliveryNotification {
    /// Wakes only after terminal publication, outside every native/app lock.
    /// Reentrancy is safe: acquisition must still be sealed by the native guard.
    pub fn notify(mut self) -> bool {
        self.notified = true;
        if self.binding.0.state.load(Ordering::Acquire) == PENDING {
            self.binding
                .0
                .listener
                .failed
                .store(true, Ordering::Release);
        }
        self.binding.0.listener.notify()
    }
}
impl Drop for WorkBrowserLeaseDeliveryNotification {
    fn drop(&mut self) {
        if !self.notified
            && self
                .binding
                .0
                .listener
                .status
                .fetch_or(NOTIFICATION_LOST, Ordering::AcqRel)
                & LISTENER_REQUIRED
                != 0
        {
            self.binding
                .0
                .listener
                .failed
                .store(true, Ordering::Release);
        }
    }
}

impl Drop for WorkBrowserLeaseDeliveryCompletion {
    fn drop(&mut self) {
        let _ = self.binding.0.state.compare_exchange(
            PENDING,
            UNPROVEN,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::task::Wake;

    fn pair() -> (
        WorkBrowserLeaseDeliveryCompletion,
        WorkBrowserLeaseDeliveryTicket,
    ) {
        let mut rows = WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
        let now = AgentPolicyInstant::from_millis(1);
        let request = rows
            .construct(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                now,
            )
            .unwrap();
        let resource = request.resource().clone();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Constructed),
                now,
            )
            .unwrap();
        let request = rows
            .acquire(
                &resource,
                ContextRunId::generate(),
                now,
                AgentPolicyInstant::from_millis(100),
            )
            .unwrap();
        let lease = request.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                now,
            )
            .unwrap();
        let (mut request, ticket) = rows.revoke_with_delivery(&lease).unwrap();
        (request.take_lease_delivery_completion().unwrap(), ticket)
    }

    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[test]
    fn delivery_registration_before_and_after_publication_has_one_wake_and_one_receipt() {
        for before in [false, true] {
            let (mut completion, mut ticket) = pair();
            let count = Arc::new(Count(AtomicUsize::new(0)));
            if before {
                ticket.register_waker(count.clone().into()).unwrap();
            }
            let notification = completion.take_notification().unwrap();
            assert!(completion.take_notification().is_none());
            assert!(ticket.try_take().unwrap().is_none());
            assert!(completion.publish_returned());
            // Atomic publication never invokes an arbitrary waker under a native guard.
            assert_eq!(count.0.load(Ordering::Acquire), 0);
            assert!(notification.notify());
            if !before {
                ticket.register_waker(count.clone().into()).unwrap();
            }
            assert_eq!(count.0.load(Ordering::Acquire), 1);
            assert!(ticket.try_take().unwrap().unwrap().returned());
            assert!(matches!(
                ticket.try_take(),
                Err(WorkBrowserLeaseDeliveryPollError::Consumed)
            ));
        }
    }

    struct PanicWake;
    impl Wake for PanicWake {
        fn wake(self: Arc<Self>) {
            panic!("injected notification failure")
        }
    }

    #[test]
    fn failed_replaced_poisoned_or_lost_notification_cannot_qualify_a_waiting_ticket() {
        for fault in 0..5 {
            let (mut completion, mut ticket) = pair();
            let count = Arc::new(Count(AtomicUsize::new(0)));
            ticket
                .register_waker(if fault == 0 {
                    Arc::new(PanicWake).into()
                } else {
                    count.clone().into()
                })
                .unwrap();
            let notification = completion.take_notification().unwrap();
            match fault {
                0 => {
                    assert!(completion.publish_returned());
                    assert!(!notification.notify());
                }
                1 => {
                    assert!(ticket.register_waker(count.into()).is_err());
                    drop(completion);
                    assert!(!notification.notify());
                }
                2 => {
                    let _ = std::panic::catch_unwind(|| {
                        let _guard = ticket.binding.0.listener.waker.lock().unwrap();
                        panic!("injected listener poison");
                    });
                    assert!(completion.publish_returned());
                    assert!(!notification.notify());
                }
                3 => {
                    assert!(completion.publish_returned());
                    drop(notification);
                }
                _ => {
                    assert!(!notification.notify());
                    drop(completion);
                }
            }
            assert!(matches!(
                ticket.try_take(),
                Err(WorkBrowserLeaseDeliveryPollError::Notification)
            ));
        }
        let (completion, mut ticket) = pair();
        ticket
            .register_waker(Arc::new(Count(AtomicUsize::new(0))).into())
            .unwrap();
        assert!(!completion.publish_returned()); // An adapter omitting the notifier is unsupported.
        assert!(matches!(
            ticket.try_take(),
            Err(WorkBrowserLeaseDeliveryPollError::Notification)
        ));
    }

    #[test]
    fn unhandled_completion_wakes_but_never_becomes_returned_evidence() {
        let (mut completion, mut ticket) = pair();
        let count = Arc::new(Count(AtomicUsize::new(0)));
        ticket.register_waker(count.clone().into()).unwrap();
        let notification = completion.take_notification().unwrap();
        drop(completion);
        assert!(notification.notify());
        assert_eq!(count.0.load(Ordering::Acquire), 1);
        assert!(!ticket.try_take().unwrap().unwrap().returned());
    }

    #[test]
    fn registration_and_notification_loss_share_one_linear_order() {
        for loss_first in [false, true] {
            let (mut completion, mut ticket) = pair();
            let notification = completion.take_notification().unwrap();
            if loss_first {
                drop(notification);
                let _ = ticket.register_waker(Arc::new(Count(AtomicUsize::new(0))).into());
            } else {
                ticket
                    .register_waker(Arc::new(Count(AtomicUsize::new(0))).into())
                    .unwrap();
                drop(notification);
            }
            drop(completion);
            assert!(matches!(
                ticket.try_take(),
                Err(WorkBrowserLeaseDeliveryPollError::Notification)
            ));
        }
    }
}
