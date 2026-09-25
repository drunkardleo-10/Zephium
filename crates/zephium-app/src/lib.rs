//! The actor shell around the pure core. One thread owns all state; commands
//! enter through a queue (UI intents and engine events alike), effects leave
//! through ports, projections go to the UI.

#[cfg(zephium_internal_repository_e2e)]
compile_error!("the internal repository E2E authority may not link into Zephium application code");

mod diagnostics;

#[cfg(all(test, feature = "work-execution-probe"))]
mod work_provider_fixture;

#[cfg(all(test, feature = "work-execution"))]
static WORK_RUNTIME_TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
#[cfg(feature = "work-execution")]
mod work;
#[cfg(feature = "agentic-browser")]
mod work_resources;

#[cfg(feature = "work-execution-probe")]
#[doc(hidden)]
pub use work_resources::probe as retained_work_probe;

#[cfg(feature = "work-execution")]
pub use work::{
    AgentWorkApplicationConfig, AgentWorkApplicationHandle, AgentWorkApplicationPhase,
    AgentWorkApplicationPorts, AgentWorkApplicationSnapshot, AgentWorkNativeFactory,
    AgentWorkReviewDecision, PreparedAgentWork,
};

pub use actor::{
    spawn, spawn_suspended, CallbackHandle, ContentPolicyStatusRequest,
    FocusedContentPolicyStatusRequest, Handle, ShutdownRequest, SpawnError, SpawnFailure,
};
#[cfg(feature = "agentic-browser")]
pub use actor::{spawn_agentic, spawn_agentic_suspended, AgenticLifecycles, AgenticSpawnFailure};
#[cfg(feature = "agentic-browser")]
pub use api::AgentLifecycle;
pub use api::{
    AcquiredExtensionCatalogSubmission, AcquiredExtensionPackageSubmission, BrowserPage,
    ChromePresentation, ChromePresentationCallback, ChromePresentationDispatch, Command,
    ContentPolicyStatusQueryOutcome, EmitFn, ExtensionLifecycle, PagePermissionPromptDecision,
    PresentationChrome, SharedBlocker, SharedChrome, SharedEngine, SharedStore,
    ShellTerminalFailure, ShellTerminalFailureCallback, ShutdownOutcome,
};
pub use shell::Shell;

#[doc(hidden)]
pub use store_reads::StoreReadResult;

pub use api::{HistoryCompletion, NoteCompletion, NotesAttachment, ResourceCompletion};
