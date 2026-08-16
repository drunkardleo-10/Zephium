//! Shell-owned consent and serialized service coordination for native grants.

use std::collections::VecDeque;

use zephium_core::extensions::ExtensionRuntimeInstance;
use zephium_core::ports::extensions::{
    ExtensionManagementAdmission, ExtensionRuntimeGrantOutcome, ExtensionRuntimeGrantPrompt,
    ExtensionRuntimeGrantPromptSettlement, ExtensionRuntimeGrantRequestId,
    MAX_PENDING_EXTENSION_RUNTIME_GRANT_REQUESTS,
};

use super::*;

const EXTENSION_RUNTIME_GRANT_TRANSACTION_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(20);

struct PendingRuntimeGrantPrompt {
    prompt: ExtensionRuntimeGrantPrompt,
    operation_id: Option<String>,
}

#[derive(Default)]
pub(super) struct ExtensionRuntimeGrantPromptState {
    pending: VecDeque<PendingRuntimeGrantPrompt>,
    failed_until_restart: bool,
}

impl ExtensionRuntimeGrantPromptState {
    fn admit(&mut self, prompt: ExtensionRuntimeGrantPrompt) -> bool {
        if self.failed_until_restart
            || self.pending.len() >= MAX_PENDING_EXTENSION_RUNTIME_GRANT_REQUESTS
            || self.pending.iter().any(|pending| {
                pending.prompt.id() == prompt.id() && pending.prompt.runtime() == prompt.runtime()
            })
        {
            return false;
        }
        self.pending.push_back(PendingRuntimeGrantPrompt {
            prompt,
            operation_id: None,
        });
        true
    }

    pub(super) fn active(&self) -> Option<(&ExtensionRuntimeGrantPrompt, bool)> {
        self.pending
            .front()
            .map(|pending| (&pending.prompt, pending.operation_id.is_some()))
    }

    fn begin_allow(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
        operation_id: String,
    ) -> Option<ExtensionRuntimeGrantPrompt> {
        let pending = self.pending.front_mut()?;
        if pending.prompt.runtime() != runtime
            || pending.prompt.id() != request
            || pending.operation_id.is_some()
        {
            return None;
        }
        pending.operation_id = Some(operation_id);
        Some(pending.prompt.clone())
    }

    fn deny(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
    ) -> Option<ExtensionRuntimeGrantPrompt> {
        let pending = self.pending.front()?;
        if pending.prompt.runtime() != runtime
            || pending.prompt.id() != request
            || pending.operation_id.is_some()
        {
            return None;
        }
        self.pending.pop_front().map(|pending| pending.prompt)
    }

    fn cancel(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
    ) -> bool {
        let Some(index) = self.pending.iter().position(|pending| {
            pending.prompt.runtime() == runtime && pending.prompt.id() == request
        }) else {
            return false;
        };
        // An admitted grant transaction cannot be cancelled after user
        // approval. Retain it until its callback reports the durable truth;
        // its later native settlement will be an inert stale response.
        if self.pending[index].operation_id.is_some() {
            return true;
        }
        self.pending.remove(index).is_some()
    }

    fn cancel_allow(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
    ) -> Option<String> {
        let pending = self.pending.front()?;
        if pending.prompt.runtime() != runtime || pending.prompt.id() != request {
            return None;
        }
        self.pending
            .pop_front()
            .and_then(|pending| pending.operation_id)
    }

    fn settle(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
    ) -> Option<String> {
        self.cancel_allow(runtime, request)
    }

    fn fail_until_restart(&mut self) {
        self.failed_until_restart = true;
    }
}

impl Shell {
    pub(super) fn on_extension_runtime_grant_prompt(
        &mut self,
        prompt: ExtensionRuntimeGrantPrompt,
    ) {
        let prompt_was_visible = self.extension_runtime_grants.active().is_some();
        if !self.extension_startup_ready
            || self.extension_lifecycle_terminal
            || self.page_permissions.has_pending()
            || !self.extension_runtime_grants.admit(prompt.clone())
        {
            let _ = self.settle_native_runtime_grant_prompt(
                &prompt,
                ExtensionRuntimeGrantPromptSettlement::Denied,
            );
            return;
        }
        if !prompt_was_visible && self.relayout() != NativeDispatch::Scheduled {
            let _ = self
                .extension_runtime_grants
                .deny(prompt.runtime(), prompt.id());
            let _ = self.settle_native_runtime_grant_prompt(
                &prompt,
                ExtensionRuntimeGrantPromptSettlement::Unavailable,
            );
            // Best-effort restoration after a partial native layout refusal.
            let _ = self.relayout();
            return;
        }
        if prompt_was_visible {
            // The visible FIFO head did not change. Avoid a projection and DOM
            // update for native requests queued behind it.
            return;
        }
        self.project_extension_runtime_grant_prompt();
    }

    pub(super) fn cancel_extension_runtime_grant_prompt(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
    ) {
        let prompt_was_visible = self.extension_runtime_grants.active().is_some();
        if self.extension_runtime_grants.cancel(runtime, request) {
            self.project_extension_runtime_grant_prompt();
            if prompt_was_visible && self.extension_runtime_grants.active().is_none() {
                let _ = self.relayout();
            }
        }
    }

    pub(super) fn begin_extension_runtime_grant_response(
        &mut self,
        operation_id: String,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
        allow: bool,
    ) -> Option<OperationDisposition> {
        if !allow {
            let prompt_was_visible = self.extension_runtime_grants.active().is_some();
            let Some(prompt) = self.extension_runtime_grants.deny(runtime, request) else {
                return Some(operation_result(
                    OperationOutcome::Rejected,
                    OperationReason::InvalidScope,
                ));
            };
            let admission = self.settle_native_runtime_grant_prompt(
                &prompt,
                ExtensionRuntimeGrantPromptSettlement::Denied,
            );
            self.project_extension_runtime_grant_prompt();
            if prompt_was_visible && self.extension_runtime_grants.active().is_none() {
                let _ = self.relayout();
            }
            return Some(if admission == NativeDispatch::Scheduled {
                operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
            } else {
                operation_result(
                    OperationOutcome::NativeAdmissionFailed,
                    OperationReason::NativeDispatchRejected,
                )
            });
        }

        if !self.extension_startup_ready || self.extension_lifecycle_terminal {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            ));
        }
        let Some(prompt) =
            self.extension_runtime_grants
                .begin_allow(runtime, request, operation_id.clone())
        else {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::InvalidScope,
            ));
        };
        self.project_extension_runtime_grant_prompt();

        let Some(queue) = self.self_queue.as_ref() else {
            return Some(self.cancel_runtime_grant_allow(
                runtime,
                request,
                OperationReason::StoreAdmissionRejected,
            ));
        };
        let callback = CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        };
        let now = std::time::Instant::now();
        let deadline = now
            .checked_add(EXTENSION_RUNTIME_GRANT_TRANSACTION_TIMEOUT)
            .unwrap_or(now);
        let Some(service) = self.extension_service.as_mut() else {
            return Some(self.cancel_runtime_grant_allow(
                runtime,
                request,
                OperationReason::StoreAdmissionRejected,
            ));
        };
        let key = prompt.key();
        let request_payload = prompt.request().clone();
        let admission = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.begin_request_runtime_grants(
                key,
                runtime.generation(),
                request_payload,
                deadline,
                Box::new(move |settlement| {
                    let _ = callback.dispatch(Command::ExtensionRuntimeGrantSettled {
                        runtime,
                        request,
                        settlement: Box::new(settlement),
                    });
                }),
            )
        }));
        match admission {
            Ok(ExtensionManagementAdmission::Accepted) => None,
            Ok(ExtensionManagementAdmission::Busy) => Some(self.cancel_runtime_grant_allow(
                runtime,
                request,
                OperationReason::StoreAdmissionRejected,
            )),
            Ok(ExtensionManagementAdmission::Unavailable) | Err(_) => {
                self.extension_runtime_grants.fail_until_restart();
                Some(self.cancel_runtime_grant_allow(
                    runtime,
                    request,
                    OperationReason::StoreReconciliationFailed,
                ))
            }
        }
    }

    fn cancel_runtime_grant_allow(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
        reason: OperationReason,
    ) -> OperationDisposition {
        let prompt_was_visible = self.extension_runtime_grants.active().is_some();
        let prompt = self
            .extension_runtime_grants
            .active()
            .filter(|(prompt, _)| prompt.runtime() == runtime && prompt.id() == request)
            .map(|(prompt, _)| prompt.clone());
        let _ = self.extension_runtime_grants.cancel_allow(runtime, request);
        if let Some(prompt) = prompt {
            let _ = self.settle_native_runtime_grant_prompt(
                &prompt,
                ExtensionRuntimeGrantPromptSettlement::Unavailable,
            );
        }
        self.project_extension_runtime_grant_prompt();
        if prompt_was_visible && self.extension_runtime_grants.active().is_none() {
            let _ = self.relayout();
        }
        operation_result(OperationOutcome::Rejected, reason)
    }

    pub(super) fn settle_extension_runtime_grant(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
        settlement: zephium_core::ports::extensions::ExtensionManagementSettlement<
            ExtensionRuntimeGrantOutcome,
        >,
    ) {
        let prompt_was_visible = self.extension_runtime_grants.active().is_some();
        let prompt = self
            .extension_runtime_grants
            .active()
            .filter(|(prompt, processing)| {
                *processing && prompt.runtime() == runtime && prompt.id() == request
            })
            .map(|(prompt, _)| prompt.clone());
        let Some(operation_id) = self.extension_runtime_grants.settle(runtime, request) else {
            crate::diagnostic!("extensions: stale runtime-grant settlement ignored");
            return;
        };
        let Some(prompt) = prompt else {
            self.extension_runtime_grants.fail_until_restart();
            return;
        };

        let (native, outcome, reason, fail_until_restart) = match settlement.into_outcome() {
            ExtensionRuntimeGrantOutcome::Granted { .. } => (
                ExtensionRuntimeGrantPromptSettlement::Granted,
                OperationOutcome::Applied,
                OperationReason::MutationApplied,
                false,
            ),
            ExtensionRuntimeGrantOutcome::AlreadyGranted { .. } => (
                ExtensionRuntimeGrantPromptSettlement::Granted,
                OperationOutcome::NoOp,
                OperationReason::StateUnchanged,
                false,
            ),
            ExtensionRuntimeGrantOutcome::Conflict => (
                ExtensionRuntimeGrantPromptSettlement::Denied,
                OperationOutcome::Rejected,
                OperationReason::StoreConflict,
                false,
            ),
            ExtensionRuntimeGrantOutcome::Rejected => (
                ExtensionRuntimeGrantPromptSettlement::Denied,
                OperationOutcome::Rejected,
                OperationReason::InvalidScope,
                false,
            ),
            ExtensionRuntimeGrantOutcome::Unavailable => (
                ExtensionRuntimeGrantPromptSettlement::Unavailable,
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
                false,
            ),
            ExtensionRuntimeGrantOutcome::OutcomeUnknown => (
                ExtensionRuntimeGrantPromptSettlement::Unavailable,
                OperationOutcome::Deferred,
                OperationReason::StoreOutcomeUnknown,
                true,
            ),
            ExtensionRuntimeGrantOutcome::FailedClosed => (
                ExtensionRuntimeGrantPromptSettlement::Unavailable,
                OperationOutcome::Rejected,
                OperationReason::StoreReconciliationFailed,
                true,
            ),
        };
        if fail_until_restart {
            self.extension_runtime_grants.fail_until_restart();
        }
        let native_admission = self.settle_native_runtime_grant_prompt(&prompt, native);
        let (outcome, reason) = if native_admission == NativeDispatch::Scheduled {
            (outcome, reason)
        } else {
            (
                OperationOutcome::NativeAdmissionFailed,
                OperationReason::NativeDispatchRejected,
            )
        };
        (self.emit)(Projection::OperationProcessed(OperationDisposition {
            operation_id,
            outcome,
            reason,
        }));
        self.project_extension_runtime_grant_prompt();
        if prompt_was_visible && self.extension_runtime_grants.active().is_none() {
            let _ = self.relayout();
        }
    }

    fn settle_native_runtime_grant_prompt(
        &self,
        prompt: &ExtensionRuntimeGrantPrompt,
        settlement: ExtensionRuntimeGrantPromptSettlement,
    ) -> NativeDispatch {
        self.engine
            .settle_extension_runtime_grant_prompt(prompt.runtime(), prompt.id(), settlement)
    }
}
