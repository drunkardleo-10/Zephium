//! Internal live metadata and sealed-tree capability registry.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use zephium_private_fs::{PrivateComponent, PrivateDirectory, SealedPrivateDirectory};

use super::names::RecordObjectKind;
use super::records::{CatalogSetRecord, PackageRecord};
use super::state::{
    MaterializationBuildIntent, MaterializationGarbageCollectionIntent, MaterializationState,
};
use crate::state::Digest32;
use crate::ExtensionRepositoryError;

pub(crate) struct MaterializationRuntime {
    pub(crate) _root: PrivateDirectory,
    pub(crate) _trees: PrivateDirectory,
    pub(crate) _records: PrivateDirectory,
    pub(crate) _journals: PrivateDirectory,
    pub(crate) _state: MaterializationState,
    pub(crate) _state_bytes: Vec<u8>,
    pub(crate) _package_records: BTreeMap<Digest32, PackageRecord>,
    pub(crate) _catalog_sets: BTreeMap<Digest32, CatalogSetRecord>,
    pub(crate) _pin_roots: MaterializationPinRoots,
    pub(crate) _sealed_tree_roots: BTreeMap<Digest32, Arc<SealedPrivateDirectory>>,
    /// Exact final inventories retained without charging dormant tree handles.
    pub(crate) _tree_object_ids: BTreeSet<Digest32>,
    pub(crate) _tree_index_ids: BTreeSet<Digest32>,
    pub(crate) _legal_artifact_ids: BTreeSet<Digest32>,
    pub(crate) _build_intent: Option<MaterializationBuildIntent>,
    pub(crate) _gc_intent: Option<MaterializationGarbageCollectionIntent>,
    pub(crate) _build_stage: Option<MaterializationTreeCapability>,
    /// Disposable acquired tree retained only while an exact acquired-package
    /// build intent owns its digest. It is never a completed-state root.
    pub(crate) _acquisition_stage: Option<MaterializationTreeCapability>,
    pub(crate) _retired_tree_ids: BTreeSet<(Digest32, u64)>,
    pub(crate) _record_stages: BTreeMap<RecordObjectKind, (Digest32, PrivateComponent)>,
}

impl MaterializationRuntime {
    /// True only when both durable intent projections exactly match recovery.
    pub(crate) fn intent_projection_is_exact(&self) -> bool {
        self._state.build_intent == self._build_intent && self._state.gc_intent == self._gc_intent
    }

    /// Physical/object writers may run only outside a collector frontier.
    pub(crate) fn garbage_collection_is_idle(&self) -> bool {
        self.validate_gc_idle_projection().is_ok()
    }

    /// Distinguishes an exact collector frontier from projection corruption.
    pub(crate) fn validate_gc_idle_projection(&self) -> Result<(), ExtensionRepositoryError> {
        if !self.intent_projection_is_exact() {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if self._gc_intent.is_some() {
            return Err(ExtensionRepositoryError::GarbageCollectionInProgress);
        }
        Ok(())
    }
}

pub(crate) enum MaterializationTreeCapability {
    Writable {
        _directory: PrivateDirectory,
    },
    Sealed {
        _directory: Arc<SealedPrivateDirectory>,
    },
}

pub(crate) struct MaterializationPinRoots {
    pub(crate) _catalog_set_ids: BTreeSet<Digest32>,
    pub(crate) _package_record_ids: BTreeSet<Digest32>,
    pub(crate) _tree_ids: BTreeSet<Digest32>,
}
