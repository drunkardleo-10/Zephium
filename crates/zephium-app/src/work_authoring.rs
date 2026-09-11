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
type ResultMessage = Result<WorkDocumentProjection, WorkError>;

#[must_use]
pub struct WorkDocumentRequest {
    receiver: Receiver<ResultMessage>,
    delivered: std::cell::Cell<bool>,
}
impl WorkDocumentRequest {
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

/// Fields and construction are private: raw Command submission cannot bypass
/// the process-wide pending/content bound.
pub(crate) struct Payload {
    pub(crate) request: WorkRequest,
    pub(crate) reply: SyncSender<ResultMessage>,
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
    pub(crate) fn take(&self) -> Option<Payload> {
        self.0.lock().ok()?.take()
    }

    pub(crate) fn prepare(request: WorkRequest) -> Result<(Self, WorkDocumentRequest), WorkError> {
        request.validate()?;
        PENDING
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 4).then_some(n + 1)
            })
            .map_err(|_| WorkError::Capacity)?;
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        Ok((
            Self(std::sync::Arc::new(std::sync::Mutex::new(Some(Payload {
                request,
                reply,
                permit: Permit,
            })))),
            WorkDocumentRequest {
                receiver,
                delivered: std::cell::Cell::new(false),
            },
        ))
    }
}
