//! The actor shell around the pure core. One thread owns all state; commands
//! enter through a queue (UI intents and engine events alike), effects leave
//! through ports, projections go to the UI.

#[cfg(zephium_internal_repository_e2e)]
compile_error!("the internal repository E2E authority may not link into Zephium application code");

mod diagnostics;

macro_rules! diagnostic {
    ($($argument:tt)*) => {{
        crate::diagnostics::write(format_args!($($argument)*));
    }};
}

pub(crate) use diagnostic;

mod actor;
mod api;
mod shell;
mod store_reads;

pub use actor::{
    spawn, spawn_suspended, CallbackHandle, ContentPolicyStatusRequest,
    FocusedContentPolicyStatusRequest, Handle, ShutdownRequest, SpawnError, SpawnFailure,
};
pub use api::{
    ChromePresentation, ChromePresentationCallback, ChromePresentationDispatch, Command,
    ContentPolicyStatusQueryOutcome, EmitFn, ExtensionLifecycle, PagePermissionPromptDecision,
    PresentationChrome, SharedBlocker, SharedChrome, SharedEngine, SharedStore,
    ShellTerminalFailure, ShellTerminalFailureCallback, ShutdownOutcome,
};
pub use shell::Shell;

#[doc(hidden)]
pub use store_reads::StoreReadResult;
