//! The actor shell around the pure core. One thread owns all state; commands
//! enter through a queue (UI intents and engine events alike), effects leave
//! through ports, projections go to the UI.

mod actor;
mod api;
mod shell;
mod store_reads;

pub use actor::{
    spawn, CallbackHandle, ContentPolicyStatusRequest, FocusedContentPolicyStatusRequest, Handle,
    ShutdownRequest, SpawnError,
};
pub use api::{
    ChromePresentation, ChromePresentationCallback, ChromePresentationDispatch, Command,
    ContentPolicyStatusQueryOutcome, EmitFn, PresentationChrome, SharedBlocker, SharedChrome,
    SharedEngine, SharedStore, ShutdownOutcome,
};
pub use shell::Shell;

#[doc(hidden)]
pub use store_reads::StoreReadResult;
