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
//! worker. Before publishing readiness, that worker settles interrupted
//! package builds, reconciles possible native owners one exact lineage at a
//! time, retires owners that still exist, releases package pins only after
//! durable native absence, and revalidates every enabled runtime before
//! activation. The unique owner exposes bounded activation, exact-runtime
//! retirement, stale-resistant enablement, and uninstall commands whose inputs
//! are identity selectors only; the worker reconstructs and revalidates Store,
//! repository, grant, and native authority inside its serialized turn. Disable
//! and uninstall retire every browsing-context owner before durable mutation;
//! enablement persists user intent before authenticated activation. Returned
//! runtime, management, and profile-retirement values are ordinary path-free,
//! non-authorizing control-flow settlements.
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

mod diagnostics;

macro_rules! diagnostic {
    ($($argument:tt)*) => {{
        crate::diagnostics::write(format_args!($($argument)*));
    }};
}

pub(crate) use diagnostic;

mod actor;
mod boot;
mod cleanup;
mod evidence;
mod journal_store;
mod mailbox;
mod manifest_projection;
mod native_recovery;
mod ports;
mod profile_retirement;
mod repository;
mod runtime_coordinator;
mod startup;
mod startup_hydration;
mod status;

const _: () = assert!(MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES > 0);

pub use actor::{
    ExtensionServiceHandle, ExtensionServiceOwner, ExtensionServiceRuntimeActivationOutcome,
    ExtensionServiceRuntimeActivationRejectionReason,
    ExtensionServiceRuntimeActivationUnavailableReason, ExtensionServiceRuntimeFailureReason,
    ExtensionServiceRuntimeRetirementOutcome, ExtensionServiceRuntimeRetirementUnavailableReason,
    ExtensionServiceSpawnError, EXTENSION_SERVICE_DEFAULT_SHUTDOWN_TIMEOUT,
};
pub use boot::{
    prepare_extension_service_boot, ExtensionServiceBootError, ExtensionServiceBootPlan,
    ExtensionServiceWorkerLaunch,
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
