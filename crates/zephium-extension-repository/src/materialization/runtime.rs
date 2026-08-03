//! Internal live metadata and sealed-tree capability registry.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use zephium_private_fs::{PrivateComponent, PrivateDirectory, SealedPrivateDirectory};

use super::names::RecordObjectKind;
use super::records::{CatalogSetRecord, PackageRecord};
use super::state::{MaterializationBuildIntent, MaterializationState};
use crate::state::Digest32;

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
    pub(crate) _build_intent: Option<MaterializationBuildIntent>,
    pub(crate) _build_stage: Option<MaterializationTreeCapability>,
    pub(crate) _retired_tree_ids: BTreeSet<(Digest32, u64)>,
    pub(crate) _record_stages: BTreeMap<RecordObjectKind, (Digest32, PrivateComponent)>,
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
