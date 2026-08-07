//! Serialized lifecycle foundation for extension coordination.
//!
//! The service owner is deliberately move-only: it can move between threads
//! but cannot be shared by reference. Cloneable handles expose observation
//! only; they cannot tear down or replace the worker. Normal actor work has a
//! fixed 1,024-entry FIFO, and shutdown owns a separate final slot so overload
//! cannot make the worker unjoinable. Profile retirement owns one additional
//! FIFO barrier slot, allowing its monotonic ingress fence to be admitted even
//! when ordinary work is saturated.
//!
//! The typed startup surface transfers a validated private-repository location,
//! the Store's unique service capability for exact runtime snapshots and
//! native ownership, and the engine's unique native-host factory into the
//! worker. Before publishing readiness, that
//! worker settles interrupted package builds, reconciles possible native
//! owners one exact lineage at a time, retires owners that still exist, and
//! releases package pins only after durable native absence. No extension
//! operation authority is exposed by this crate yet. Profile-retirement
//! results are ordinary non-authorizing control-flow settlements.
//!
//! The owner cannot be shared across threads:
//!
//! ```compile_fail
//! use zephium_extension_service::ExtensionServiceOwner;
//!
//! fn assert_sync<T: Sync>() {}
//! assert_sync::<ExtensionServiceOwner>();
//! ```
//!
//! The owner is not cloneable:
//!
//! ```compile_fail
//! use zephium_extension_service::ExtensionServiceOwner;
//!
//! fn require_clone<T: Clone>() {}
//! require_clone::<ExtensionServiceOwner>();
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub(crate) use zephium_extension_runtime_api::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES;

mod actor;
mod cleanup;
mod evidence;
mod journal_store;
mod mailbox;
mod native_recovery;
mod ports;
mod profile_retirement;
mod repository;
#[allow(dead_code)] // Worker ingress wiring lands after the private coordinator is validated.
mod runtime_coordinator;
mod startup;
mod status;

const _: () = assert!(MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES > 0);

pub use actor::{
    ExtensionServiceHandle, ExtensionServiceOwner, ExtensionServiceSpawnError,
    EXTENSION_SERVICE_DEFAULT_SHUTDOWN_TIMEOUT,
};
pub use evidence::{
    ExtensionServiceCleanupEvidence, ExtensionServiceReadyEvidence,
    ExtensionServiceShutdownEvidence, ExtensionServiceWorkerIdentity,
};
pub use mailbox::{EXTENSION_SERVICE_MAILBOX_CAPACITY, EXTENSION_SERVICE_NORMAL_CAPACITY};
pub use ports::{ExtensionServiceShutdownOutcome, ExtensionServiceStatusPort};
pub use profile_retirement::{
    ExtensionServiceProfileRetirementFailureReason, ExtensionServiceProfileRetirementOutcome,
    ExtensionServiceProfileRetirementUnavailableReason,
};
pub use startup::{
    ExtensionRepositoryRoot, ExtensionRepositoryRootError, ExtensionServiceLaunchInput,
    ExtensionServiceStartupFailure, ExtensionServiceStartupFailureReason,
    ExtensionServiceStartupOutcome, ExtensionServiceStartupUnavailable,
    ExtensionServiceStartupUnavailableReason, ExtensionServiceStartupWait,
    EXTENSION_REPOSITORY_DIRECTORY_NAME,
};
pub use status::{
    ExtensionServicePhase, ExtensionServiceStatusSnapshot, ExtensionServiceStatusWait,
};
