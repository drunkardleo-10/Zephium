//! Explicit trusted Rust admission only; no Tauri IPC or UI entry point.

use std::sync::{Arc, Mutex};
use tauri::Manager;
use zephium_app::{AgentWorkApplicationHandle, PreparedAgentWork};
use zephium_work_composition::{MacosWorkComposition, TrustedWorkRequest};

/// Nonblocking selected-session query for an ordinary Work invocation. Wait for
/// Ready before passing that exact binding to launch. Selecting a browser
/// profile does not identify an authenticated service account.
pub fn selected_work_profile(
    app: &tauri::AppHandle,
) -> Result<zephium_app::AgentWorkProfileRequest, WorkAdmissionFailure> {
    let shell = app
        .try_state::<zephium_app::Handle>()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    Ok(shell.work_profile_binding())
}

/// Launches a product-approved natural-language objective through the retained
/// browser runtime. Drain `take_event` for live progress, inspect `snapshot`,
/// and move the source-bound terminal result with `take_extraction`. Draining
/// events releases bounded runtime backpressure. Queue admission is not success.
pub fn launch_public_read_work(
    app: &tauri::AppHandle,
    profile: zephium_app::AgentWorkProfileBinding,
    invocation: zephium_work_composition::PublicReadWorkInvocation,
) -> Result<zephium_app::RetainedWorkHandle, WorkAdmissionFailure> {
    let request = invocation
        .into_request(profile)
        .map_err(WorkAdmissionFailure::Contract)?;
    admit_retained_trusted_work(app, request)
}

/// Launches a product-approved public objective with independently assessed,
/// reversible local actions. The trusted Rust invocation owns the action policy;
/// neither the UI nor this adapter can manufacture refs or widen its effects.
pub fn launch_public_local_action_work(
    app: &tauri::AppHandle,
    profile: zephium_app::AgentWorkProfileBinding,
    invocation: zephium_work_composition::PublicLocalActionWorkInvocation,
) -> Result<zephium_app::RetainedWorkHandle, WorkAdmissionFailure> {
    let request = invocation
        .into_request(profile)
        .map_err(WorkAdmissionFailure::Contract)?;
    admit_retained_trusted_work(app, request)
}

pub(crate) struct WorkCompositionState(Mutex<CompositionAdmission>);

struct CompositionAdmission {
    composition: Option<MacosWorkComposition>,
    initial_attached: bool,
}

pub(crate) fn install(
    app: &tauri::AppHandle,
    engine: Arc<zephium_engine::WebviewEngine>,
    store: Arc<zephium_store::SqliteStore>,
) -> bool {
    app.manage(WorkCompositionState(Mutex::new(CompositionAdmission {
        composition: Some(MacosWorkComposition::new(engine, store)),
        initial_attached: false,
    })))
}

/// Typed local admission refusal. No native authority has been recreated.
pub enum WorkAdmissionFailure {
    /// Desktop startup or this process's one-shot composition is unavailable.
    Unavailable,
    /// Trusted input did not meet the existing controller contract.
    Contract(zephium_work_composition::AgentWorkFailure),
    /// Attachment was not queued. Both original non-executing owners remain
    /// available for an explicit attachment/admission retry, without preparation.
    AttachmentMailbox {
        /// Original prepared task and deferred native factory.
        prepared: Box<PreparedAgentWork>,
        /// Exact engine/Store owners needed to retry attachment.
        composition: MacosWorkComposition,
    },
    /// The exact prepared input remains owned after shell mailbox refusal.
    Mailbox {
        /// Original non-executing prepared owner.
        prepared: Box<PreparedAgentWork>,
        /// Exact attached coordinator for a caller-decided admission retry.
        handle: AgentWorkApplicationHandle,
    },
}

impl std::fmt::Debug for WorkAdmissionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("WorkAdmissionFailure::Unavailable"),
            Self::Contract(error) => f
                .debug_tuple("WorkAdmissionFailure::Contract")
                .field(error)
                .finish(),
            Self::AttachmentMailbox { .. } => {
                f.write_str("WorkAdmissionFailure::AttachmentMailbox([owned])")
            }
            Self::Mailbox { .. } => f.write_str("WorkAdmissionFailure::Mailbox([owned])"),
        }
    }
}

/// Accepts only an already-approved Rust task/effect/account contract. This is
/// deliberately not a Tauri command and is unavailable in default desktop builds.
pub fn admit_trusted_work(
    app: &tauri::AppHandle,
    request: TrustedWorkRequest,
) -> Result<AgentWorkApplicationHandle, WorkAdmissionFailure> {
    admit_after(app, request, None)
}

/// Launches one approved read-only objective on an application-retained page.
/// This is trusted Rust composition only, never Tauri IPC. The original Shell
/// rechecks profile and owners; the returned handle reports actual admission.
pub fn admit_retained_trusted_work(
    app: &tauri::AppHandle,
    request: TrustedWorkRequest,
) -> Result<zephium_app::RetainedWorkHandle, WorkAdmissionFailure> {
    let shell = app
        .try_state::<zephium_app::Handle>()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let state = app
        .try_state::<WorkCompositionState>()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let mut owner = state
        .0
        .lock()
        .map_err(|_| WorkAdmissionFailure::Unavailable)?;
    if owner.initial_attached {
        return Err(WorkAdmissionFailure::Unavailable);
    }
    let composition = owner
        .composition
        .as_ref()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let handle = composition
        .launch_retained(&shell.callback_handle(), request)
        .map_err(WorkAdmissionFailure::Contract)?
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    owner.initial_attached = true;
    Ok(handle)
}

#[cfg(feature = "macos-work-retained-product-probe")]
pub(crate) fn retained_resource_failure_cause(
    app: &tauri::AppHandle,
    view: &zephium_app::RetainedWorkHandle,
) -> Option<zephium_engine::WorkResourceFailureCause> {
    let state = app.try_state::<WorkCompositionState>()?;
    let owner = state.0.lock().ok()?;
    owner
        .composition
        .as_ref()?
        .retained_resource_failure_cause(view)
}

/// Explicit fresh trusted work after an exact completed predecessor. This is
/// not resume/retry permission; the Shell and native factory independently
/// retain all original closure, output-drain and lifetime checks.
pub fn admit_successor_trusted_work(
    app: &tauri::AppHandle,
    predecessor: &AgentWorkApplicationHandle,
    request: TrustedWorkRequest,
) -> Result<AgentWorkApplicationHandle, WorkAdmissionFailure> {
    admit_after(app, request, Some(predecessor))
}

fn admit_after(
    app: &tauri::AppHandle,
    request: TrustedWorkRequest,
    predecessor: Option<&AgentWorkApplicationHandle>,
) -> Result<AgentWorkApplicationHandle, WorkAdmissionFailure> {
    let shell = app
        .try_state::<zephium_app::Handle>()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let state = app
        .try_state::<WorkCompositionState>()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let mut owner = state
        .0
        .lock()
        .map_err(|_| WorkAdmissionFailure::Unavailable)?;
    if predecessor.is_some() != owner.initial_attached {
        return Err(WorkAdmissionFailure::Unavailable);
    }
    let (composition, prepared) = prepare_owned(&mut owner.composition, |composition| {
        composition.prepare(request)
    })
    .map_err(|error| match error {
        PreparationFailure::Unavailable => WorkAdmissionFailure::Unavailable,
        PreparationFailure::Contract(error) => WorkAdmissionFailure::Contract(error),
    })?;
    let view = match predecessor {
        Some(predecessor) => composition.attach_successor(&shell.callback_handle(), predecessor),
        None => composition.attach(&shell.callback_handle()),
    };
    let Some(view) = view else {
        return Err(WorkAdmissionFailure::AttachmentMailbox {
            prepared: Box::new(prepared),
            composition,
        });
    };
    // Retain the exact process-unique factory owner for an explicit successor;
    // initial admission remains one-shot and failed preparation changes neither.
    owner.composition = Some(composition);
    owner.initial_attached = true;
    if let Err(prepared) = view.admit(prepared) {
        return Err(WorkAdmissionFailure::Mailbox {
            prepared,
            handle: view,
        });
    }
    Ok(view)
}

enum PreparationFailure<E> {
    Unavailable,
    Contract(E),
}

/// The only slot transition: validation completes before taking the owner.
/// This has no shell/Store access and is called under the admission mutex.
fn prepare_owned<C, P, E>(
    slot: &mut Option<C>,
    prepare: impl FnOnce(&C) -> Result<P, E>,
) -> Result<(C, P), PreparationFailure<E>> {
    let composition = slot.as_ref().ok_or(PreparationFailure::Unavailable)?;
    let prepared = prepare(composition).map_err(PreparationFailure::Contract)?;
    let composition = slot.take().ok_or(PreparationFailure::Unavailable)?;
    Ok((composition, prepared))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_preparation_preserves_the_exact_slot_for_later_valid_admission() {
        let identity = Arc::new(());
        let mut slot = Some(identity.clone());
        let mut attachments = 0;
        let invalid = prepare_owned(&mut slot, |_| Err::<(), _>("stale"));
        if invalid.is_ok() {
            attachments += 1;
        }
        assert!(matches!(
            invalid,
            Err(PreparationFailure::Contract("stale"))
        ));
        assert!(Arc::ptr_eq(slot.as_ref().unwrap(), &identity));
        assert_eq!(attachments, 0);

        let prepared = Box::new(7);
        let exact = std::ptr::from_ref(prepared.as_ref());
        let Ok((owner, prepared)) = prepare_owned(&mut slot, |_| Ok::<_, ()>(prepared)) else {
            panic!("valid preparation must retain admission");
        };
        attachments += 1;
        assert!(Arc::ptr_eq(&owner, &identity));
        assert_eq!(std::ptr::from_ref(prepared.as_ref()), exact);
        assert!(slot.is_none());
        assert_eq!(attachments, 1);
        assert!(matches!(
            prepare_owned(&mut slot, |_| -> Result<(), ()> { panic!("consumed slot") }),
            Err(PreparationFailure::Unavailable)
        ));
    }
}
