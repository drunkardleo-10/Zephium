//! Optional nonblocking Work lane on the existing Store actor, without workers.

#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

use super::{Cmd, SqliteStore};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, OnceLock, TryLockError,
};
use zephium_agentic::{
    AgentWorkJournalCompletion, AgentWorkJournalError as Error, AgentWorkJournalPort,
    AgentWorkJournalRequest,
};

const MAX_PENDING_WORK_WRITES: usize = 4;

pub(super) struct WorkPermit(Arc<AtomicUsize>);

impl WorkPermit {
    fn acquire(slot: &OnceLock<Arc<AtomicUsize>>) -> Result<Self, Error> {
        let counter = slot.get_or_init(|| Arc::new(AtomicUsize::new(0))).clone();
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending
                    .checked_add(1)
                    .filter(|value| *value <= MAX_PENDING_WORK_WRITES)
            })
            .map_err(|_| Error::Capacity)?;
        Ok(Self(counter))
    }
}

impl Drop for WorkPermit {
    fn drop(&mut self) {
        if self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending.checked_sub(1)
            })
            .is_err()
        {
            self.0.store(usize::MAX, Ordering::Release);
        }
    }
}

impl AgentWorkJournalPort for SqliteStore {
    fn artifact(
        &self,
        request: zephium_agentic::AgentWorkArtifactRequest,
        completion: zephium_agentic::AgentWorkArtifactCompletion,
    ) -> Result<(), Error> {
        let lifecycle = match self.lifecycle.try_read() {
            Ok(value) => value,
            Err(TryLockError::Poisoned(value)) => value.into_inner(),
            Err(TryLockError::WouldBlock) => {
                return refuse_artifact(Error::Unavailable, completion)
            }
        };
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return refuse_artifact(Error::Shutdown, completion);
        }
        let permit = match WorkPermit::acquire(&self.work_admission) {
            Ok(permit) => permit,
            Err(error) => return refuse_artifact(error, completion),
        };
        match self
            .tx
            .try_send(Cmd::AgentWorkArtifact(request, permit, completion))
        {
            Ok(()) => Ok(()),
            Err(error) => {
                let (error, command) = match error {
                    mpsc::TrySendError::Full(command) => (Error::Capacity, command),
                    mpsc::TrySendError::Disconnected(command) => (Error::Shutdown, command),
                };
                if let Cmd::AgentWorkArtifact(_, _, completion) = command {
                    return refuse_artifact(error, completion);
                }
                Err(error)
            }
        }
    }
    fn dispatch(
        &self,
        request: AgentWorkJournalRequest,
        completion: AgentWorkJournalCompletion,
    ) -> Result<(), Error> {
        let lifecycle = match self.lifecycle.try_read() {
            Ok(value) => value,
            Err(TryLockError::Poisoned(value)) => value.into_inner(),
            Err(TryLockError::WouldBlock) => return refuse(Error::Unavailable, completion),
        };
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return refuse(Error::Shutdown, completion);
        }
        let permit = match WorkPermit::acquire(&self.work_admission) {
            Ok(permit) => permit,
            Err(error) => return refuse(error, completion),
        };
        match self
            .tx
            .try_send(Cmd::AgentWork(request, permit, completion))
        {
            Ok(()) => Ok(()),
            Err(error) => {
                let (error, command) = match error {
                    mpsc::TrySendError::Full(command) => (Error::Capacity, command),
                    mpsc::TrySendError::Disconnected(command) => (Error::Shutdown, command),
                };
                if let Cmd::AgentWork(_, _, completion) = command {
                    return refuse(error, completion);
                }
                Err(error)
            }
        }
    }
}

fn refuse(error: Error, completion: AgentWorkJournalCompletion) -> Result<(), Error> {
    let _discard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(completion)));
    Err(error)
}

fn refuse_artifact(
    error: Error,
    completion: zephium_agentic::AgentWorkArtifactCompletion,
) -> Result<(), Error> {
    let _discard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(completion)));
    Err(error)
}

pub(super) fn settle_artifact(
    hub: &mut crate::hub::Hub,
    request: zephium_agentic::AgentWorkArtifactRequest,
    completion: zephium_agentic::AgentWorkArtifactCompletion,
) {
    let result = hub.agent_work_artifact(request);
    let _completed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(result)));
}

pub(super) fn settle(
    hub: &mut crate::hub::Hub,
    request: AgentWorkJournalRequest,
    completion: AgentWorkJournalCompletion,
) {
    let result = hub.agent_work(request);
    // A foreign callback (or destructor) cannot kill the shared Store worker.
    let _completed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(result)));
}
