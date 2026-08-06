//! Serialized lifecycle foundation for extension coordination.
//!
//! The service owner is deliberately move-only: it can move between threads
//! but cannot be shared by reference. Cloneable handles expose observation
//! only; they cannot tear down or replace the worker. Normal actor work has a
//! fixed 1,024-entry FIFO, and shutdown owns a separate final slot so overload
//! cannot make the worker unjoinable.
//!
//! The typed startup surface transfers a validated private-repository location
//! and the Store's unique native-ownership capability into the worker. Before
//! publishing readiness, that worker settles interrupted package builds and
//! reconciles every durable row whose native owner is definitely absent.
//! Possible-owner rows are retained and reported as cleanup-required; no
//! extension operation authority is exposed by this crate yet.
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

mod actor;
mod cleanup;
mod evidence;
mod journal_store;
mod mailbox;
mod ports;
mod repository;
mod startup;
mod status;

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
