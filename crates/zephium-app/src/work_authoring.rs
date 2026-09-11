//! Rust product intent entry point. Work content uses bounded, on-demand Store
//! requests; no browser-execution admission or model transport is initialized.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{Receiver, SyncSender, TryRecvError},
};
use zephium_core::{
    ids::ProfileId,
    work::{port::*, WorkError},
};

static PENDING: AtomicUsize = AtomicUsize::new(0);
pub(crate) struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        PENDING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The profile is the application-selected owner at dispatch. A frontend must
/// match it to its current surface before rendering a delayed response.
#[derive(Debug)]
pub struct WorkDocumentProjection {
    pub profile: ProfileId,
    pub reply: WorkReply,
}
type Wake = std::sync::Arc<std::sync::Mutex<Option<std::task::Waker>>>;
type ResultMessage = Result<WorkDocumentProjection, WorkError>;

#[must_use]
pub struct WorkDocumentRequest {
    owner: std::sync::Arc<std::sync::OnceLock<ProfileId>>,
    work_id: Option<zephium_core::work::WorkId>,
    receiver: Receiver<ResultMessage>,
    delivered: std::cell::Cell<bool>,
    wake: Wake,
}
impl WorkDocumentRequest {
    pub async fn response(self, profile: ProfileId) -> zephium_ipc::work::WorkResponseV1 {
        let result = match self.await {
            Ok(value) if value.profile == profile => Ok(value.reply),
            Ok(_) => Err(WorkError::ProfileUnavailable),
            Err(error) => Err(error),
        };
        zephium_ipc::work::WorkResponseV1::from_result(profile, result)
    }
    /// Selected by Shell before Store dispatch, including when the final
    /// callback is lost. Reconcile under this owner, not the newly focused tab.
    pub fn profile(&self) -> Option<ProfileId> {
        self.owner.get().copied()
    }
    /// Retained even if the callback is lost, so a newly minted Work can be
    /// reconciled by identity without replaying creation.
    pub fn work_id(&self) -> Option<zephium_core::work::WorkId> {
        self.work_id
    }
    /// No polling worker is owned by this request. A disconnected accepted
    /// request is uncertain, including shutdown before callback delivery.
    pub fn try_recv(&self) -> Option<ResultMessage> {
        if self.delivered.get() {
            return None;
        }
        let result = match self.receiver.try_recv() {
            Ok(value) => value,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(WorkError::OutcomeUnknown),
        };
        self.delivered.set(true);
        Some(result)
    }
}

impl Drop for WorkDocumentRequest {
    fn drop(&mut self) {
        // A Store callback may outlive its observer; do not retain that
        // observer's task through its registered waker until Store shutdown.
        let wake = self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(wake);
    }
}

impl std::future::Future for WorkDocumentRequest {
    type Output = ResultMessage;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut wake = self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.try_recv() {
            Some(value) => std::task::Poll::Ready(value),
            None => {
                *wake = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct WorkReplySender {
    sender: Option<SyncSender<ResultMessage>>,
    wake: Wake,
}
impl WorkReplySender {
    pub(crate) fn try_send(&self, value: ResultMessage) {
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(value);
        }
        self.notify();
    }
    fn notify(&self) {
        let wake = self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
impl Drop for WorkReplySender {
    fn drop(&mut self) {
        drop(self.sender.take());
        self.notify();
    }
}

/// Fields and construction are private: raw Command submission cannot bypass
/// the process-wide pending/content bound.
pub(crate) struct Payload {
    pub(crate) owner: std::sync::Arc<std::sync::OnceLock<ProfileId>>,
    pub(crate) request: WorkRequest,
    pub(crate) expected_owner: Option<ProfileId>,
    pub(crate) pinned_owner: bool,
    pub(crate) reply: WorkReplySender,
    pub(crate) permit: Permit,
}
#[derive(Clone)]
pub struct WorkDocumentSubmission(std::sync::Arc<std::sync::Mutex<Option<Payload>>>);
impl std::fmt::Debug for WorkDocumentSubmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkDocumentSubmission([owned, redacted])")
    }
}
impl WorkDocumentSubmission {
    #[cfg(feature = "work-runtime")]
    pub(crate) fn prepare_pinned(
        request: WorkRequest,
        profile: ProfileId,
    ) -> Result<(Self, WorkDocumentRequest), WorkError> {
        if !matches!(
            request,
            WorkRequest::RuntimeRead { .. }
                | WorkRequest::ReadEvidence { .. }
                | WorkRequest::RuntimeUpdate { .. }
                | WorkRequest::RuntimeAbandon { .. }
        ) {
            return Err(WorkError::Invalid);
        }
        let prepared = Self::prepare_bound(request, Some(profile))?;
        prepared
            .0
             .0
            .lock()
            .map_err(|_| WorkError::Unavailable)?
            .as_mut()
            .ok_or(WorkError::Unavailable)?
            .pinned_owner = true;
        Ok(prepared)
    }
    pub(crate) fn take(&self) -> Option<Payload> {
        self.0.lock().ok()?.take()
    }

    pub(crate) fn prepare_bound(
        request: WorkRequest,
        expected_owner: Option<ProfileId>,
    ) -> Result<(Self, WorkDocumentRequest), WorkError> {
        request.validate()?;
        let work_id = match &request {
            WorkRequest::AuthoringCommand { intent, .. } => intent.work(),
            WorkRequest::Create { id, .. }
            | WorkRequest::ReadEvidence { id, .. }
            | WorkRequest::RuntimeAbandon { id, .. }
            | WorkRequest::RuntimeRead { id }
            | WorkRequest::RuntimeCommand { id, .. }
            | WorkRequest::RuntimeUpdate { id, .. }
            | WorkRequest::Edit { id, .. }
            | WorkRequest::Read { id }
            | WorkRequest::ReadPlan { id, .. }
            | WorkRequest::ListPlans { id }
            | WorkRequest::Delete { id, .. } => Some(*id),
            WorkRequest::List { .. } => None,
        };
        PENDING
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 4).then_some(n + 1)
            })
            .map_err(|_| WorkError::Capacity)?;
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let wake = std::sync::Arc::new(std::sync::Mutex::new(None));
        let reply = WorkReplySender {
            sender: Some(reply),
            wake: wake.clone(),
        };
        let owner = std::sync::Arc::new(std::sync::OnceLock::new());
        Ok((
            Self(std::sync::Arc::new(std::sync::Mutex::new(Some(Payload {
                owner: owner.clone(),
                request,
                expected_owner,
                pinned_owner: false,
                reply,
                permit: Permit,
            })))),
            WorkDocumentRequest {
                owner,
                work_id,
                receiver,
                delivered: std::cell::Cell::new(false),
                wake,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::Pin,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };
    struct Counter(AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn work_document_dropped_future_releases_registered_observer() {
        let (submission, mut receipt) =
            WorkDocumentSubmission::prepare_bound(WorkRequest::Read { id: 1.into() }, None)
                .unwrap();
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        assert!(Pin::new(&mut receipt)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        assert_eq!(Arc::strong_count(&counter), 3);
        drop(receipt);
        assert_eq!(Arc::strong_count(&counter), 2);
        drop(submission);
        assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn work_document_future_wakes_for_success_and_lost_last_sender() {
        for lost in [false, true] {
            let (submission, mut receipt) =
                WorkDocumentSubmission::prepare_bound(WorkRequest::Read { id: 1.into() }, None)
                    .unwrap();
            let counter = Arc::new(Counter(AtomicUsize::new(0)));
            let waker = Waker::from(counter.clone());
            let mut context = Context::from_waker(&waker);
            assert!(Pin::new(&mut receipt).poll(&mut context).is_pending());
            if lost {
                drop(submission);
            } else {
                submission
                    .take()
                    .unwrap()
                    .reply
                    .try_send(Err(WorkError::NotFound));
            }
            assert!(counter.0.load(Ordering::SeqCst) > 0);
            assert!(
                matches!(Pin::new(&mut receipt).poll(&mut context),Poll::Ready(Err(error)) if error==if lost {WorkError::OutcomeUnknown} else {WorkError::NotFound})
            );
            assert!(receipt.try_recv().is_none());
        }
    }
}
