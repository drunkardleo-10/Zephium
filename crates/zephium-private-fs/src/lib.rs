//! Small, fail-closed filesystem primitives for browser-owned private data.
//!
//! This crate owns the platform-specific boundary checks shared by durable
//! browser subsystems. It deliberately does not own any subsystem's journal,
//! recovery, garbage-collection, or package-admission policy.
//!
//! Phase-one namespace activation is available only on macOS and Linux, where
//! child operations are descriptor-relative. Other Unix targets and Windows
//! fail closed until their native adapters have passed dedicated live tests.
//!
//! The boundary excludes unprivileged operating-system principals that have no
//! delegated access and rejects untrusted package contents. Root/administrator,
//! malicious code already running with the same user authority, and mutation
//! rights delegated through an accepted ancestor ACL are outside the guarantee;
//! the lease lock only serializes cooperating Zephium processes. Exact identity
//! checks detect observed path redirection and stickily quarantine the lease.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod component;
mod entry_name;
mod error;
mod identity;
mod lease;
mod namespace;
mod platform;

pub use component::{PrivateComponent, PrivateComponentError};
pub use entry_name::{PrivateEntryName, PrivateEntryNameError, MAX_PRIVATE_ENTRY_NAME_BYTES};
pub use error::PrivateFsError;
pub use identity::{DirectoryIdentity, FileIdentity};
pub use namespace::{ByteLimit, LockedPrivateNamespace, PrivateDirectory};

/// Hard ceiling for any allocation or write accepted by the in-memory API.
///
/// Larger artifacts must use a separately reviewed streaming interface. This
/// prevents a caller-provided limit from silently becoming an unbounded memory
/// allocation.
pub const MAX_IN_MEMORY_FILE_BYTES: usize = 128 * 1024 * 1024;
