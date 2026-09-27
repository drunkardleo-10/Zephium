//! One-shot Shell relay for a native isolated extension document's package
//! resource. The service actor verifies bytes; Shell owns no package path.

use super::*;
use zephium_core::extensions::ExtensionRuntimeInstance;
use zephium_core::ports::extensions::{
    ExtensionManagementAdmission, IsolatedExtensionResourceOutcome,
    IsolatedExtensionResourceRequest,
};

impl Shell {
    pub(super) fn on_isolated_extension_resource_request(
        &mut self,
        request: IsolatedExtensionResourceRequest,
    ) {
        let runtime = request.runtime();
        let kind = request.kind();
        let id = request.id();
        if !self.bootstrapped || self.profile_deletion_quarantines(runtime.profile()) {
            let _ = self.engine.settle_isolated_extension_resource(
                runtime,
                kind,
                id,
                IsolatedExtensionResourceOutcome::RuntimeUnavailable,
            );
            return;
        }
        let Some(queue) = self.self_queue.as_ref() else {
            let _ = self.engine.settle_isolated_extension_resource(
                runtime,
                kind,
                id,
                IsolatedExtensionResourceOutcome::WorkerUnavailable,
            );
            return;
        };
        let callback = CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        };
        let Some(service) = self.extension_service.as_mut() else {
            let _ = self.engine.settle_isolated_extension_resource(
                runtime,
                kind,
                id,
                IsolatedExtensionResourceOutcome::WorkerUnavailable,
            );
            return;
        };
        let admission = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.begin_read_isolated_resource(
                runtime,
                request.path().into(),
                request.deadline(),
                request.cancel_token(),
                Box::new(move |outcome| {
                    let _ = callback.dispatch(Command::IsolatedExtensionResourceSettled {
                        runtime,
                        kind,
                        request: id,
                        outcome: Arc::new(Mutex::new(Some(outcome))),
                    });
                }),
            )
        }));
        let rejection = match admission {
            Ok(ExtensionManagementAdmission::Accepted) => return,
            Ok(ExtensionManagementAdmission::Busy) => IsolatedExtensionResourceOutcome::Capacity,
            Ok(ExtensionManagementAdmission::Unavailable) | Err(_) => {
                IsolatedExtensionResourceOutcome::WorkerUnavailable
            }
        };
        let _ = self
            .engine
            .settle_isolated_extension_resource(runtime, kind, id, rejection);
    }

    pub(super) fn settle_isolated_extension_resource(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        kind: zephium_core::ports::extensions::IsolatedExtensionDocumentKind,
        id: u64,
        outcome: Arc<Mutex<Option<IsolatedExtensionResourceOutcome>>>,
    ) {
        let outcome = outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(outcome) = outcome {
            let _ = self
                .engine
                .settle_isolated_extension_resource(runtime, kind, id, outcome);
        }
    }
}
