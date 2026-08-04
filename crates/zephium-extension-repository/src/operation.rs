//! Shared high-level repository operation serialization and health.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
use std::sync::{mpsc::Sender, TryLockError};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::ExtensionRepositoryError;

std::thread_local! {
    static EXTERNAL_CALLBACK_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Repository-wide runtime state shared with detached package leases.
#[derive(Clone)]
pub(crate) struct RepositoryRuntime {
    inner: Arc<RepositoryRuntimeInner>,
}

struct RepositoryRuntimeInner {
    operation_gate: RepositoryOperationGate,
    health: RepositoryHealth,
}

impl RepositoryRuntime {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(RepositoryRuntimeInner {
                operation_gate: RepositoryOperationGate::new(),
                health: RepositoryHealth::new(),
            }),
        }
    }

    pub(crate) fn enter(&self) -> Result<RepositoryOperationGuard<'_>, RepositoryOperationError> {
        self.inner.operation_gate.enter(&self.inner.health)
    }

    pub(crate) fn is_healthy(&self) -> bool {
        self.inner.health.is_healthy()
    }

    pub(crate) fn poison(&self) {
        self.inner.health.poison();
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    pub(crate) fn arm_contention_probe(&self, probe: Sender<()>) {
        self.inner.operation_gate.arm_contention_probe(probe);
    }
}

/// Runs one trusted adapter callback while prohibiting repository re-entry on
/// this thread. The previous state makes nested adapter boundaries panic-safe.
pub(crate) fn with_external_callback<T>(callback: impl FnOnce() -> T) -> T {
    let previous = EXTERNAL_CALLBACK_ACTIVE.with(|active| active.replace(true));
    let _scope = ExternalCallbackScope { previous };
    callback()
}

pub(crate) fn reject_if_external_callback() -> Result<(), ExtensionRepositoryError> {
    if external_callback_is_active() {
        Err(ExtensionRepositoryError::CallbackReentry)
    } else {
        Ok(())
    }
}

fn external_callback_is_active() -> bool {
    EXTERNAL_CALLBACK_ACTIVE.with(Cell::get)
}

struct ExternalCallbackScope {
    previous: bool,
}

impl Drop for ExternalCallbackScope {
    fn drop(&mut self) {
        EXTERNAL_CALLBACK_ACTIVE.with(|active| active.set(self.previous));
    }
}

struct RepositoryHealth(AtomicBool);

impl RepositoryHealth {
    fn new() -> Self {
        Self(AtomicBool::new(true))
    }

    fn is_healthy(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn poison(&self) {
        self.0.store(false, Ordering::Release);
    }
}

struct RepositoryOperationGate {
    operation: Mutex<()>,
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    contention_probe: Mutex<Option<Sender<()>>>,
}

impl RepositoryOperationGate {
    fn new() -> Self {
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

    fn enter<'a>(
        &'a self,
        health: &RepositoryHealth,
    ) -> Result<RepositoryOperationGuard<'a>, RepositoryOperationError> {
        if reject_if_external_callback().is_err() {
            return Err(RepositoryOperationError::CallbackReentry);
        }
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
    fn arm_contention_probe(&self, probe: Sender<()>) {
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
    CallbackReentry,
    Unhealthy,
    Poisoned,
}

impl RepositoryOperationError {
    pub(crate) const fn repository_error(self) -> ExtensionRepositoryError {
        match self {
            Self::CallbackReentry => ExtensionRepositoryError::CallbackReentry,
            Self::Unhealthy => ExtensionRepositoryError::Sealed,
            Self::Poisoned => ExtensionRepositoryError::SettlementAmbiguous,
        }
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn external_callbacks_reject_all_nested_repository_gates_without_poisoning() {
        let first = RepositoryRuntime::new();
        let second = RepositoryRuntime::new();

        with_external_callback(|| {
            assert!(matches!(
                first.enter(),
                Err(RepositoryOperationError::CallbackReentry)
            ));
            assert!(matches!(
                second.enter(),
                Err(RepositoryOperationError::CallbackReentry)
            ));
        });

        assert!(first.is_healthy());
        assert!(second.is_healthy());
        assert!(first.enter().is_ok());
    }

    #[test]
    fn nested_callback_scopes_restore_the_outer_prohibition() {
        let runtime = RepositoryRuntime::new();
        with_external_callback(|| {
            with_external_callback(|| {
                assert!(matches!(
                    runtime.enter(),
                    Err(RepositoryOperationError::CallbackReentry)
                ));
            });
            assert!(matches!(
                runtime.enter(),
                Err(RepositoryOperationError::CallbackReentry)
            ));
        });
        assert!(runtime.enter().is_ok());
    }

    #[test]
    fn panicking_callback_restores_repository_entry() {
        let runtime = RepositoryRuntime::new();
        let result = std::panic::catch_unwind(|| {
            with_external_callback(|| panic!("adapter callback panic"));
        });
        assert!(result.is_err());
        assert!(runtime.enter().is_ok());
    }
}
