//! Shared high-level repository operation serialization and health.

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
use std::sync::{mpsc::Sender, TryLockError};
use std::sync::{Mutex, MutexGuard};

use crate::ExtensionRepositoryError;

pub(crate) struct RepositoryHealth(AtomicBool);

impl RepositoryHealth {
    pub(crate) fn new() -> Self {
        Self(AtomicBool::new(true))
    }

    pub(crate) fn is_healthy(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(crate) fn poison(&self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) struct RepositoryOperationGate {
    operation: Mutex<()>,
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    contention_probe: Mutex<Option<Sender<()>>>,
}

impl RepositoryOperationGate {
    pub(crate) fn new() -> Self {
        Self {
            operation: Mutex::new(()),
            #[cfg(all(
                test,
                zephium_internal_repository_e2e,
                any(target_os = "macos", target_os = "linux")
            ))]
            contention_probe: Mutex::new(None),
        }
    }

    pub(crate) fn enter<'a>(
        &'a self,
        health: &RepositoryHealth,
    ) -> Result<RepositoryOperationGuard<'a>, RepositoryOperationError> {
        #[cfg(all(
            test,
            zephium_internal_repository_e2e,
            any(target_os = "macos", target_os = "linux")
        ))]
        match self.operation.try_lock() {
            Ok(guard) if health.is_healthy() => {
                return Ok(RepositoryOperationGuard { _guard: guard });
            }
            Ok(_guard) => return Err(RepositoryOperationError::Unhealthy),
            Err(TryLockError::Poisoned(_poisoned)) => {
                health.poison();
                return Err(RepositoryOperationError::Poisoned);
            }
            Err(TryLockError::WouldBlock) => self.notify_contention_probe(),
        }

        match self.operation.lock() {
            Ok(guard) if health.is_healthy() => Ok(RepositoryOperationGuard { _guard: guard }),
            Ok(_guard) => Err(RepositoryOperationError::Unhealthy),
            Err(_poisoned) => {
                health.poison();
                Err(RepositoryOperationError::Poisoned)
            }
        }
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    pub(crate) fn arm_contention_probe(&self, probe: Sender<()>) {
        match self.contention_probe.lock() {
            Ok(mut slot) => assert!(slot.replace(probe).is_none()),
            Err(_poisoned) => panic!("repository operation contention probe was poisoned"),
        }
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    fn notify_contention_probe(&self) {
        let probe = match self.contention_probe.lock() {
            Ok(mut slot) => slot.take(),
            Err(_poisoned) => panic!("repository operation contention probe was poisoned"),
        };
        if let Some(probe) = probe {
            let _ = probe.send(());
        }
    }
}

pub(crate) struct RepositoryOperationGuard<'a> {
    _guard: MutexGuard<'a, ()>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepositoryOperationError {
    Unhealthy,
    Poisoned,
}

impl RepositoryOperationError {
    pub(crate) const fn repository_error(self) -> ExtensionRepositoryError {
        match self {
            Self::Unhealthy => ExtensionRepositoryError::Sealed,
            Self::Poisoned => ExtensionRepositoryError::SettlementAmbiguous,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn poisoned_gate_stickily_poisoned_repository_health() {
        let gate = Arc::new(RepositoryOperationGate::new());
        let health = Arc::new(RepositoryHealth::new());
        let worker_gate = Arc::clone(&gate);
        let worker_health = Arc::clone(&health);
        assert!(std::thread::spawn(move || {
            let _operation = worker_gate.enter(&worker_health).unwrap();
            panic!("poison operation gate");
        })
        .join()
        .is_err());

        assert!(matches!(
            gate.enter(&health),
            Err(RepositoryOperationError::Poisoned)
        ));
        assert!(!health.is_healthy());
    }
}
