//! Explicit trusted Rust admission only; no Tauri IPC or UI entry point.

use std::sync::{Arc, Mutex};
use tauri::Manager;
use zephium_app::{AgentWorkApplicationHandle, PreparedAgentWork};
use zephium_work_composition::{MacosWorkComposition, TrustedWorkRequest};

pub(crate) struct WorkCompositionState(Mutex<Option<MacosWorkComposition>>);

pub(crate) fn install(
    app: &tauri::AppHandle,
    engine: Arc<zephium_engine::WebviewEngine>,
    store: Arc<zephium_store::SqliteStore>,
) -> bool {
    app.manage(WorkCompositionState(Mutex::new(Some(
        MacosWorkComposition::new(engine, store),
    ))))
}

/// Typed local admission refusal. No native authority has been recreated.
pub enum WorkAdmissionFailure {
    /// Desktop startup or this process's one-shot composition is unavailable.
    Unavailable,
    /// Trusted input did not meet the existing controller contract.
    Contract(zephium_work_composition::AgentWorkFailure),
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
    let shell = app
        .try_state::<zephium_app::Handle>()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let state = app
        .try_state::<WorkCompositionState>()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let composition = state
        .0
        .lock()
        .map_err(|_| WorkAdmissionFailure::Unavailable)?
        .take()
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let view = composition
        .attach(&shell.callback_handle())
        .ok_or(WorkAdmissionFailure::Unavailable)?;
    let prepared = composition
        .prepare(request)
        .map_err(WorkAdmissionFailure::Contract)?;
    if let Err(prepared) = view.admit(prepared) {
        return Err(WorkAdmissionFailure::Mailbox {
            prepared,
            handle: view,
        });
    }
    Ok(view)
}
