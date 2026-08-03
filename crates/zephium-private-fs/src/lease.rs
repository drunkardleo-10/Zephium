use std::fs::File;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::AtomicU8;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::platform;
use crate::{DirectoryIdentity, FileIdentity, PrivateComponent, PrivateFsError};

pub(crate) struct NamespaceLease {
    _lock: File,
    lock_identity: FileIdentity,
    reserved_lock: PrivateComponent,
    reserved_lock_staging: PrivateComponent,
    root_path: PathBuf,
    root_handle: File,
    root_identity: DirectoryIdentity,
    operation: Mutex<()>,
    quarantined: AtomicBool,
    #[cfg(all(test, unix))]
    fail_next_settlement: AtomicBool,
    #[cfg(test)]
    committed_mutation_faults: AtomicU8,
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum CommittedMutationFault {
    Remove = 1 << 0,
    Replace = 1 << 1,
    PublishNoreplace = 1 << 2,
    CreateDirectory = 1 << 3,
}

impl NamespaceLease {
    pub(crate) fn new(
        lock: File,
        lock_identity: FileIdentity,
        reserved_lock: PrivateComponent,
        reserved_lock_staging: PrivateComponent,
        root_path: PathBuf,
        root_handle: File,
        root_identity: DirectoryIdentity,
    ) -> Self {
        Self {
            _lock: lock,
            lock_identity,
            reserved_lock,
            reserved_lock_staging,
            root_path,
            root_handle,
            root_identity,
            operation: Mutex::new(()),
            quarantined: AtomicBool::new(false),
            #[cfg(all(test, unix))]
            fail_next_settlement: AtomicBool::new(false),
            #[cfg(test)]
            committed_mutation_faults: AtomicU8::new(0),
        }
    }

    pub(crate) fn begin(&self) -> Result<MutexGuard<'_, ()>, PrivateFsError> {
        if self.quarantined.load(Ordering::Acquire) {
            return Err(PrivateFsError::Quarantined);
        }
        let guard = self.operation.lock().map_err(|_| {
            self.quarantined.store(true, Ordering::Release);
            PrivateFsError::Quarantined
        })?;
        if self.quarantined.load(Ordering::Acquire) {
            return Err(PrivateFsError::Quarantined);
        }
        Ok(guard)
    }

    pub(crate) fn is_reserved_name(&self, name: &str) -> bool {
        self.is_reserved_lock_name(name) || self.is_reserved_lock_staging_name(name)
    }

    pub(crate) fn is_reserved_lock_name(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case(self.reserved_lock.as_str())
    }

    pub(crate) fn is_canonical_reserved_lock_name(&self, name: &str) -> bool {
        name == self.reserved_lock.as_str()
    }

    pub(crate) fn is_reserved_lock_staging_name(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case(self.reserved_lock_staging.as_str())
    }

    pub(crate) fn verify_authority(&self) -> Result<(), PrivateFsError> {
        if !platform::same_open_identity(&self.root_handle, self.root_identity.0) {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        let (current_root, identity) = platform::open_directory(&self.root_path)
            .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
        if identity != self.root_identity.0
            || !platform::same_open_identity(&current_root, identity)
        {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        platform::revalidate_regular(
            &self.root_handle,
            &self.root_path,
            self.reserved_lock.as_str(),
            &self._lock,
            self.lock_identity.0,
        )
        .map_err(|_| PrivateFsError::IdentityAmbiguous)
    }

    pub(crate) fn observe<T>(
        &self,
        result: Result<T, PrivateFsError>,
    ) -> Result<T, PrivateFsError> {
        if matches!(result, Err(PrivateFsError::IdentityAmbiguous)) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }

    pub(crate) fn settle_after_commit<T>(
        &self,
        result: Result<T, PrivateFsError>,
    ) -> Result<T, PrivateFsError> {
        #[cfg(all(test, unix))]
        let result = if self.fail_next_settlement.swap(false, Ordering::AcqRel) {
            Err(PrivateFsError::Io)
        } else {
            result
        };
        match result {
            Ok(value) => Ok(value),
            Err(_) => {
                self.quarantined.store(true, Ordering::Release);
                Err(PrivateFsError::SettlementUnknown)
            }
        }
    }

    #[cfg(all(test, unix))]
    pub(crate) fn fail_next_settlement(&self) {
        self.fail_next_settlement.store(true, Ordering::Release);
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    pub(crate) fn inject_committed_mutation_fault(&self, fault: CommittedMutationFault) {
        self.committed_mutation_faults
            .fetch_or(fault as u8, Ordering::AcqRel);
    }

    #[cfg(test)]
    pub(crate) fn take_committed_mutation_fault(&self, fault: CommittedMutationFault) -> bool {
        let mask = fault as u8;
        self.committed_mutation_faults
            .fetch_and(!mask, Ordering::AcqRel)
            & mask
            != 0
    }
}
