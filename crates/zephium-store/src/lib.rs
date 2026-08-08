//! Bounded, crash-aware SQLite persistence behind one dedicated actor.

mod actor;
mod bounded_json;
mod hub;
mod legacy;
mod migrations;
mod pane;
#[cfg(target_os = "windows")]
mod windows_file_identity;

pub use actor::{
    ExtensionServiceStoreAuthority, ExtensionServiceStoreAuthorityClaimError,
    ExtensionServiceStoreCallOutcome, ExtensionServiceStoreStartupRequirement, SqliteStore,
};
