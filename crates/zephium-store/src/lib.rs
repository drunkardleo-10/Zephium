//! Bounded, crash-aware SQLite persistence behind one dedicated actor.

mod actor;
mod bounded_json;
mod hub;
pub use hub::media::{admit as admit_media, MediaAdmissionError, MediaStore};
mod legacy;
mod migrations;
mod pane;
#[cfg(target_os = "windows")]
mod windows_file_identity;

pub use actor::{
    ExtensionRuntimeStartupInventory, ExtensionRuntimeStartupInventoryLoadOutcome,
    ExtensionServiceStoreAuthority, ExtensionServiceStoreAuthorityClaimError,
    ExtensionServiceStoreCallOutcome, ExtensionServiceStoreStartupRequirement, SqliteStore,
};

mod work_frames;
mod work_migration_v2;
pub use work_frames::{WorkFrameRecord, WorkFrameStore};
