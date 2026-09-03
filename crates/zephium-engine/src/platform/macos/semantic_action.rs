#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

//! Fixed macOS semantic-action adapter.
//!
//! Exact target revalidation and the closed click recipe both execute inside
//! the immutable isolated runtime. The result remains provisional until the
//! core's fresh semantic postcondition verification succeeds. Engine-native
//! responder delivery is intentionally not admitted here: physical evidence
//! shows that it grants page user activation and therefore needs a separate
//! capability-boundary and presentation authority.

use std::time::Instant;

use zephium_agentic::{
    encode_semantic_action_runtime_invocation, SemanticActionExecutionInstant, SemanticActionKind,
    SemanticActionNativeFailure, SemanticActionNativeRequest, SemanticActionNativeSettlement,
    SemanticActionRuntimeFault, SemanticActionRuntimeResultError,
};

use super::semantic_runtime::{
    AgentSemanticActionRuntimeFailure, AgentSemanticRuntimeController,
    AgentSemanticRuntimeDispatchError,
};

pub(super) fn dispatch(
    _view: &wry::WebView,
    semantic: &AgentSemanticRuntimeController,
    request: SemanticActionNativeRequest,
    admitted_at: Instant,
    completion: impl FnOnce(SemanticActionNativeSettlement) + 'static,
) {
    if request.kind() != SemanticActionKind::Click {
        let completed_at = failure_instant(
            &request,
            admitted_at,
            SemanticActionNativeFailure::UnsupportedInteraction,
        );
        completion(request.fail(
            SemanticActionNativeFailure::UnsupportedInteraction,
            completed_at,
        ));
        return;
    }
    let invocation = match encode_semantic_action_runtime_invocation(&request) {
        Ok(invocation) => invocation,
        Err(_) => {
            let completed_at = failure_instant(
                &request,
                admitted_at,
                SemanticActionNativeFailure::Transport,
            );
            completion(request.fail(SemanticActionNativeFailure::Transport, completed_at));
            return;
        }
    };
    let _ = semantic.dispatch_action(invocation, move |outcome| {
        let settlement = match outcome {
            Ok(evidence) => complete_fixed_recipe(request, evidence, admitted_at),
            Err(failure) => {
                let native = map_runtime_failure(failure);
                let completed_at = failure_instant(&request, admitted_at, native);
                request.fail(native, completed_at)
            }
        };
        completion(settlement);
    });
}

fn complete_fixed_recipe(
    request: SemanticActionNativeRequest,
    evidence: zephium_agentic::SemanticActionRuntimeEvidence,
    admitted_at: Instant,
) -> SemanticActionNativeSettlement {
    let Some(completed_at) = mapped_instant(request.requested_at(), admitted_at, Instant::now())
    else {
        let deadline = request.deadline();
        return request.fail(SemanticActionNativeFailure::TimedOut, deadline);
    };
    if completed_at > request.deadline() {
        let deadline = request.deadline();
        return request.fail(SemanticActionNativeFailure::TimedOut, deadline);
    }
    request.complete(
        evidence.backend(),
        evidence.readiness(),
        evidence.viewport(),
        evidence.geometry(),
        completed_at,
        completed_at,
    )
}

fn mapped_instant(
    requested_at: SemanticActionExecutionInstant,
    admitted_at: Instant,
    now: Instant,
) -> Option<SemanticActionExecutionInstant> {
    let elapsed = u64::try_from(now.saturating_duration_since(admitted_at).as_millis()).ok()?;
    mapped_elapsed(requested_at, elapsed)
}

fn mapped_elapsed(
    requested_at: SemanticActionExecutionInstant,
    elapsed_millis: u64,
) -> Option<SemanticActionExecutionInstant> {
    requested_at
        .millis()
        .checked_add(elapsed_millis)
        .map(SemanticActionExecutionInstant::from_millis)
}

fn failure_instant(
    request: &SemanticActionNativeRequest,
    admitted_at: Instant,
    failure: SemanticActionNativeFailure,
) -> SemanticActionExecutionInstant {
    if failure == SemanticActionNativeFailure::TimedOut {
        return request.deadline();
    }
    mapped_instant(request.requested_at(), admitted_at, Instant::now())
        .unwrap_or(request.deadline())
}

const fn map_dispatch_failure(
    failure: AgentSemanticRuntimeDispatchError,
) -> SemanticActionNativeFailure {
    match failure {
        AgentSemanticRuntimeDispatchError::NotReady => SemanticActionNativeFailure::TargetChanged,
        AgentSemanticRuntimeDispatchError::Busy => SemanticActionNativeFailure::ResourceExhausted,
        AgentSemanticRuntimeDispatchError::Exhausted => {
            SemanticActionNativeFailure::ResourceExhausted
        }
        AgentSemanticRuntimeDispatchError::Retired => SemanticActionNativeFailure::Shutdown,
    }
}

const fn map_runtime_failure(
    failure: AgentSemanticActionRuntimeFailure,
) -> SemanticActionNativeFailure {
    match failure {
        AgentSemanticActionRuntimeFailure::Dispatch(failure) => map_dispatch_failure(failure),
        AgentSemanticActionRuntimeFailure::Cancelled => SemanticActionNativeFailure::Cancelled,
        AgentSemanticActionRuntimeFailure::DocumentReplaced => {
            SemanticActionNativeFailure::StaleReference
        }
        AgentSemanticActionRuntimeFailure::RendererLost => {
            SemanticActionNativeFailure::RendererLost
        }
        AgentSemanticActionRuntimeFailure::TimedOut => SemanticActionNativeFailure::TimedOut,
        AgentSemanticActionRuntimeFailure::Retired => SemanticActionNativeFailure::Shutdown,
        AgentSemanticActionRuntimeFailure::Transport => SemanticActionNativeFailure::Transport,
        AgentSemanticActionRuntimeFailure::Result(SemanticActionRuntimeResultError::Runtime(
            fault,
        )) => map_runtime_fault(fault),
        AgentSemanticActionRuntimeFailure::Result(
            SemanticActionRuntimeResultError::OutputLimit
            | SemanticActionRuntimeResultError::InvalidFault
            | SemanticActionRuntimeResultError::InvalidEncoding
            | SemanticActionRuntimeResultError::Correlation
            | SemanticActionRuntimeResultError::InvalidGeometry,
        ) => SemanticActionNativeFailure::Transport,
    }
}

const fn map_runtime_fault(fault: SemanticActionRuntimeFault) -> SemanticActionNativeFailure {
    match fault {
        SemanticActionRuntimeFault::StaleReference => SemanticActionNativeFailure::StaleReference,
        SemanticActionRuntimeFault::TargetChanged => SemanticActionNativeFailure::TargetChanged,
        SemanticActionRuntimeFault::TargetDisabled => SemanticActionNativeFailure::TargetDisabled,
        SemanticActionRuntimeFault::CredentialBoundary => {
            SemanticActionNativeFailure::CredentialBoundary
        }
        SemanticActionRuntimeFault::TargetOccluded => SemanticActionNativeFailure::TargetOccluded,
        SemanticActionRuntimeFault::UnsupportedInteraction => {
            SemanticActionNativeFailure::UnsupportedInteraction
        }
        SemanticActionRuntimeFault::Busy => SemanticActionNativeFailure::ResourceExhausted,
        SemanticActionRuntimeFault::DocumentLoading => SemanticActionNativeFailure::TargetChanged,
        SemanticActionRuntimeFault::InvalidRequest | SemanticActionRuntimeFault::Internal => {
            SemanticActionNativeFailure::Transport
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_clock_mapping_is_checked_and_monotonic() {
        let requested = SemanticActionExecutionInstant::from_millis(1_000);
        assert_eq!(
            mapped_elapsed(requested, 25).map(|value| value.millis()),
            Some(1_025)
        );
        assert_eq!(
            mapped_elapsed(SemanticActionExecutionInstant::from_millis(u64::MAX), 1),
            None
        );
    }
}
