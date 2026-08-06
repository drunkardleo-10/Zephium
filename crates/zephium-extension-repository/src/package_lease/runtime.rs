//! Lease-local snapshot sharing, owner presence, and repository-open epochs.

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use zephium_core::ids::{ExtensionInstallId, ProfileId};

use crate::materialization::{
    OwnerPackagePinIdentity, PackageLeaseRepositoryIdentity, VerifiedActivePackageSnapshot,
    VerifiedRollbackPackageSnapshot, MAX_COMPLETED_PACKAGE_RECORDS, MAX_DURABLE_PACKAGE_PINS,
};
use crate::state::Digest32;

pub(super) struct RepositoryOpenEpoch;

pub(super) struct LeasePresence {
    pub(super) open_epoch: Arc<RepositoryOpenEpoch>,
    pub(super) repository: PackageLeaseRepositoryIdentity,
    pub(super) profile: ProfileId,
    pub(super) install: ExtensionInstallId,
    pub(super) binding: LeasePresenceBinding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LeasePresenceBinding {
    DurablePin(OwnerPackagePinIdentity),
    Reconciliation,
}

type LeaseOwner = (ProfileId, ExtensionInstallId);

pub(crate) struct PackageLeaseRuntime {
    open_epoch: Arc<RepositoryOpenEpoch>,
    pub(super) live: BTreeMap<LeaseOwner, Weak<LeasePresence>>,
    active_cache: BTreeMap<Digest32, Weak<VerifiedActivePackageSnapshot>>,
    rollback_cache: BTreeMap<Digest32, Weak<VerifiedRollbackPackageSnapshot>>,
}

impl PackageLeaseRuntime {
    pub(crate) fn new() -> Self {
        Self {
            open_epoch: Arc::new(RepositoryOpenEpoch),
            live: BTreeMap::new(),
            active_cache: BTreeMap::new(),
            rollback_cache: BTreeMap::new(),
        }
    }

    pub(super) fn open_epoch(&self) -> &Arc<RepositoryOpenEpoch> {
        &self.open_epoch
    }

    pub(super) fn share_active(
        &mut self,
        fresh: VerifiedActivePackageSnapshot,
    ) -> Result<Arc<VerifiedActivePackageSnapshot>, LocalLeaseError> {
        self.active_cache
            .retain(|_, value| value.strong_count() != 0);
        let record_id = fresh.record_id();
        if let Some(cached) = self.active_cache.get(&record_id).and_then(Weak::upgrade) {
            if !cached.exactly_matches(&fresh) {
                return Err(LocalLeaseError::SnapshotMismatch);
            }
            return Ok(cached);
        }
        if self.active_cache.len() >= MAX_COMPLETED_PACKAGE_RECORDS {
            return Err(LocalLeaseError::CapacityExhausted);
        }
        let shared = Arc::new(fresh);
        self.active_cache.insert(record_id, Arc::downgrade(&shared));
        Ok(shared)
    }

    pub(super) fn share_rollback(
        &mut self,
        fresh: VerifiedRollbackPackageSnapshot,
    ) -> Result<Arc<VerifiedRollbackPackageSnapshot>, LocalLeaseError> {
        self.rollback_cache
            .retain(|_, value| value.strong_count() != 0);
        let record_id = fresh.record_id();
        if let Some(cached) = self.rollback_cache.get(&record_id).and_then(Weak::upgrade) {
            if !cached.exactly_matches(&fresh) {
                return Err(LocalLeaseError::SnapshotMismatch);
            }
            return Ok(cached);
        }
        if self.rollback_cache.len() >= MAX_COMPLETED_PACKAGE_RECORDS {
            return Err(LocalLeaseError::CapacityExhausted);
        }
        let shared = Arc::new(fresh);
        self.rollback_cache
            .insert(record_id, Arc::downgrade(&shared));
        Ok(shared)
    }

    pub(super) fn reserve_owner(
        &mut self,
        repository: PackageLeaseRepositoryIdentity,
        pin: OwnerPackagePinIdentity,
    ) -> Result<Arc<LeasePresence>, LocalLeaseError> {
        let (profile, install) = pin.lease_owner();
        self.reserve(
            repository,
            profile,
            install,
            LeasePresenceBinding::DurablePin(pin),
        )
    }

    pub(super) fn reserve_reconciliation(
        &mut self,
        repository: PackageLeaseRepositoryIdentity,
        profile: ProfileId,
        install: ExtensionInstallId,
    ) -> Result<Arc<LeasePresence>, LocalLeaseError> {
        self.reserve(
            repository,
            profile,
            install,
            LeasePresenceBinding::Reconciliation,
        )
    }

    fn reserve(
        &mut self,
        repository: PackageLeaseRepositoryIdentity,
        profile: ProfileId,
        install: ExtensionInstallId,
        binding: LeasePresenceBinding,
    ) -> Result<Arc<LeasePresence>, LocalLeaseError> {
        self.live.retain(|_, value| value.strong_count() != 0);
        let owner = (profile, install);
        if self.live.get(&owner).and_then(Weak::upgrade).is_some() {
            return Err(LocalLeaseError::AlreadyOpen);
        }
        self.live.remove(&owner);
        if self.live.len() >= MAX_DURABLE_PACKAGE_PINS {
            return Err(LocalLeaseError::CapacityExhausted);
        }
        let presence = Arc::new(LeasePresence {
            open_epoch: Arc::clone(&self.open_epoch),
            repository,
            profile,
            install,
            binding,
        });
        self.live.insert(owner, Arc::downgrade(&presence));
        Ok(presence)
    }

    pub(super) fn install_release_presence(
        &mut self,
        presence: &Arc<LeasePresence>,
    ) -> Result<(), LocalLeaseError> {
        self.live.retain(|_, value| value.strong_count() != 0);
        let owner = (presence.profile, presence.install);
        match self.live.get(&owner).and_then(Weak::upgrade) {
            Some(current) if Arc::ptr_eq(&current, presence) => Ok(()),
            Some(_) => Err(LocalLeaseError::ConcurrentLease),
            None => {
                if self.live.len() >= MAX_DURABLE_PACKAGE_PINS {
                    return Err(LocalLeaseError::ConcurrentLease);
                }
                self.live.insert(owner, Arc::downgrade(presence));
                Ok(())
            }
        }
    }

    pub(super) fn retire_presence(&mut self, presence: &Arc<LeasePresence>) {
        let owner = (presence.profile, presence.install);
        if self
            .live
            .get(&owner)
            .and_then(Weak::upgrade)
            .is_some_and(|current| Arc::ptr_eq(&current, presence))
        {
            self.live.remove(&owner);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LocalLeaseError {
    AlreadyOpen,
    ConcurrentLease,
    SnapshotMismatch,
    CapacityExhausted,
}
