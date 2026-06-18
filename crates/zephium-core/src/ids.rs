//! Syncable aggregates use ULID (sortable, global); ephemeral entities use u64.

use ulid::Ulid;

pub type SyncId = Ulid;
pub type LocalId = u64;
