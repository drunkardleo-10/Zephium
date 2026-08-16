//! Low-frequency, non-blocking extension-repository maintenance.
//!
//! Shell contributes only its existing heartbeat. The extension-service
//! worker owns coalescing, repository serialization, and the bounded batch;
//! this module never creates a timer, worker, native view, or retry loop.

use zephium_core::ports::extensions::{
    ExtensionRepositoryMaintenanceAdmission, ExtensionRepositoryMaintenanceOutcome,
};

use super::*;

const EXTENSION_REPOSITORY_MAINTENANCE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(5);

impl Shell {
    pub(super) fn maintain_extension_repository(&mut self) {
        if !self.extension_startup_ready
            || self.extension_lifecycle_terminal
            || self.extension_repository_maintenance_failed_closed
        {
            return;
        }
        let Some(service) = self.extension_service.as_mut() else {
            self.extension_repository_maintenance_failed_closed = true;
            crate::diagnostic!("extensions: repository maintenance lifecycle owner is missing");
            return;
        };
        let available = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.repository_maintenance_is_available()
        }));
        match available {
            Ok(false) => return,
            Ok(true) => {}
            Err(_) => {
                self.extension_repository_maintenance_failed_closed = true;
                crate::diagnostic!(
                    "extensions: repository maintenance availability check panicked"
                );
                return;
            }
        }
        let Some(queue) = self.self_queue.as_ref() else {
            self.extension_repository_maintenance_failed_closed = true;
            crate::diagnostic!("extensions: repository maintenance callback queue is missing");
            return;
        };
        let callback = CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        };
        let now = std::time::Instant::now();
        let deadline = now
            .checked_add(EXTENSION_REPOSITORY_MAINTENANCE_TIMEOUT)
            .unwrap_or(now);
        let admission = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.begin_repository_maintenance(
                deadline,
                Box::new(move |outcome| {
                    let _ =
                        callback.dispatch(Command::ExtensionRepositoryMaintenanceSettled(outcome));
                }),
            )
        }));
        match admission {
            Ok(
                ExtensionRepositoryMaintenanceAdmission::Accepted
                | ExtensionRepositoryMaintenanceAdmission::Pending
                | ExtensionRepositoryMaintenanceAdmission::Busy,
            ) => {}
            Ok(ExtensionRepositoryMaintenanceAdmission::Unavailable) | Err(_) => {
                self.extension_repository_maintenance_failed_closed = true;
                crate::diagnostic!("extensions: repository maintenance admission failed closed");
            }
        }
    }

    pub(super) fn settle_extension_repository_maintenance(
        &mut self,
        outcome: ExtensionRepositoryMaintenanceOutcome,
    ) {
        match outcome {
            ExtensionRepositoryMaintenanceOutcome::NoGarbage
            | ExtensionRepositoryMaintenanceOutcome::Collected { .. } => {}
            ExtensionRepositoryMaintenanceOutcome::Unavailable => {
                // Retain the ordinary one-minute heartbeat as the only retry
                // authority. Maintenance never creates a hot follow-up even
                // when another bounded batch is known to exist.
                crate::diagnostic!("extensions: repository maintenance is temporarily unavailable");
            }
            ExtensionRepositoryMaintenanceOutcome::FailedClosed => {
                self.extension_repository_maintenance_failed_closed = true;
                crate::diagnostic!("extensions: repository maintenance failed closed");
            }
        }
    }
}
