//! Durable metadata and opaque sealed-root recovery for materialized packages.
//!
//! This module deliberately has no public byte source, materialization writer,
//! activation operation, garbage collector, or receipt constructor. Opening a
//! repository recovers only bounded metadata and live sealed tree-root
//! identities. Payload files remain unread and unenumerated until a later
//! authority-bearing operation explicitly verifies them.

mod interlock;
mod names;
mod records;
mod recovery;
mod runtime;
mod state;
mod storage;

pub(crate) use interlock::validate_catalog_advance;
pub(crate) use records::{
    MAX_CATALOG_SET_PACKAGES, MAX_CATALOG_SET_RECORD_BYTES, MAX_PACKAGE_RECORD_BYTES,
};
pub(crate) use recovery::{is_pristine_for_outer_initialization, open_or_recover, FaultPoint};
pub(crate) use runtime::MaterializationRuntime;
pub(crate) use state::{
    MAX_COMPLETED_PACKAGE_RECORDS, MAX_DURABLE_PACKAGE_PINS, MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    MAX_MATERIALIZATION_JOURNAL_BYTES, MAX_MATERIALIZATION_STATE_BYTES,
};

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(crate) use records::tests::package_record_fixture;
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(crate) use state::{MaterializationBuildIntent, MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION};
