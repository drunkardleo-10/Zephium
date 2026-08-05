//! Serialized lifecycle foundation for extension coordination.
//!
//! The service owner is deliberately move-only: it can move between threads
//! but cannot be shared by reference. Cloneable handles expose observation
//! only; they cannot tear down or replace the worker. Normal actor work has a
//! fixed 1,024-entry FIFO, and shutdown owns a separate final slot so overload
//! cannot make the worker unjoinable.
//!
//! This crate does not yet expose extension operations or accept repository,
//! Store, or native-runtime authority. Those inputs will enter only through a
//! typed actor boundary once their recovery protocol is implemented.
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
//! let owner = ExtensionServiceOwner::spawn().unwrap();
//! let _duplicate = owner.clone();
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod actor;
mod evidence;
mod mailbox;
mod ports;
mod status;

pub use actor::{
    ExtensionServiceHandle, ExtensionServiceOwner, ExtensionServiceSpawnError,
    EXTENSION_SERVICE_DEFAULT_SHUTDOWN_TIMEOUT,
};
pub use evidence::{ExtensionServiceShutdownEvidence, ExtensionServiceWorkerIdentity};
pub use mailbox::{EXTENSION_SERVICE_MAILBOX_CAPACITY, EXTENSION_SERVICE_NORMAL_CAPACITY};
pub use ports::{ExtensionServiceShutdownOutcome, ExtensionServiceStatusPort};
pub use status::{
    ExtensionServicePhase, ExtensionServiceStatusSnapshot, ExtensionServiceStatusWait,
};
