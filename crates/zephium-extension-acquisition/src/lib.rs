//! Bounded authentication and streaming access for acquired extension archives.
//!
//! This crate is the untrusted-package boundary between transport and durable
//! extension materialization. It authenticates a CRX3 developer identity and
//! the exact catalog-bound ZIP payload before allowing the ZIP parser to see
//! the archive. A small allocation-free end-record preflight bounds central
//! directory allocation first; complete entry preflight then rejects aliases,
//! links, encryption, unsupported compression, oversized expansion, and local
//! header ambiguity before a single payload byte can be copied.
//!
//! It deliberately performs no network access, filesystem traversal, release
//! catalog admission, profile mutation, or native activation. The resulting
//! value is not package authority. A repository must still copy every file
//! into a fresh private stage, construct and authenticate the canonical tree,
//! admit its manifest, and atomically publish the package record.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod archive;

pub use archive::{
    AcquiredExtensionArchive, AcquiredExtensionArchiveError, AcquiredExtensionArchiveFile,
    AcquiredExtensionArchiveReadError, MAX_ACQUIRED_ARCHIVE_CENTRAL_DIRECTORY_BYTES,
    MAX_ACQUIRED_ARCHIVE_RETAINED_BYTES,
};
