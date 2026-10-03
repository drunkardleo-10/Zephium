//! Non-shipping owner for a fresh fixed KnownFolder QA session.
use super::super::{identity, native, security, sync_directory};
use super::*;

/// A fresh QA test session with original full identity and bounded cleanup.
/// Children reopen its label; only the creating parent owns retirement.
pub struct NativeWorkStorageTestSession {
    session: NativeSession,
    original: DirectoryIdentity,
    retired: bool,
}

impl NativeWorkStorageTestSession {
    /// Exclusively creates a new session through the actual fixed Work route.
    /// A preexisting session is never adopted as a disposable test fixture.
    pub fn create(session: &NativeSession) -> Result<Self, PrivateFsError> {
        let profile = known_profile()?;
        let anchor = NativeStorageAnchor::prepare_under_fresh_session(&profile, session)?;
        let original = anchor.identity();
        drop(anchor);
        Ok(Self {
            session: session.clone(),
            original,
            retired: false,
        })
    }

    /// Stable closed session identity for child and Store constructors.
    #[must_use]
    pub fn session(&self) -> &NativeSession {
        &self.session
    }

    /// Consumes the owner after every Store/process lease has been dropped.
    /// An active owner, changed identity or unexpected residue refuses cleanup.
    pub fn retire(mut self) -> Result<(), PrivateFsError> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> Result<(), PrivateFsError> {
        let profile = known_profile()?;
        let expected = profile
            .join(ANCHOR)
            .join(NativeApplication::ExtensionQa.component())
            .join("qa-sessions")
            .join(format!("session-{}", self.session.0));
        // Open every directory and the lease; never recreate missing residue.
        let anchor = NativeStorageAnchor::prepare_under_existing_session(&profile, &self.session)?;
        if anchor.path != expected || anchor.identity() != self.original {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        anchor.verify()?;
        let parent = &anchor
            .directories
            .last()
            .ok_or(PrivateFsError::Unsafe)?
            .file;
        let entries = native::names(parent, 4)?;
        for name in &entries {
            if !["work.sqlite", "work.sqlite-wal", "work.sqlite-shm", LEASE]
                .contains(&name.as_str())
            {
                return Err(PrivateFsError::Unsafe);
            }
        }
        // Original root pins prohibit redirecting these fixed leaf operations.
        // Unlinking a hostile reparse leaf removes only the owned link itself.
        for name in entries {
            if name != LEASE {
                std::fs::remove_file(expected.join(name)).map_err(|_| PrivateFsError::Unsafe)?;
            }
        }
        let lock_identity = anchor.lock_identity;
        drop(anchor.lock);
        let lock = native::open(
            parent,
            LEASE,
            Some(false),
            native::READ | native::METADATA,
            7,
            native::OPEN,
            None,
        )?;
        verify_lock(&lock)?;
        if identity(&lock, Some(false))?.0 != lock_identity {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        native::delete(&lock)?;
        drop(lock);
        let mut directories = anchor.directories;
        let leaf = directories.pop().ok_or(PrivateFsError::Unsafe)?;
        let name = leaf.name.clone().ok_or(PrivateFsError::Unsafe)?;
        drop(leaf);
        let parent = &directories.last().ok_or(PrivateFsError::Unsafe)?.file;
        let exact = native::open(
            parent,
            &name,
            Some(true),
            native::READ | native::METADATA,
            7,
            native::OPEN,
            None,
        )?;
        native::exact_name(&exact, &name)?;
        if DirectoryIdentity(identity(&exact, Some(true))?.0) != self.original {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        security::native_storage::verify(&exact)?;
        native::delete(&exact)?;
        drop(exact);
        sync_directory(parent)?;
        match std::fs::symlink_metadata(expected) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.retired = true;
                Ok(())
            }
            _ => Err(PrivateFsError::SettlementUnknown),
        }
    }
}

impl Drop for NativeWorkStorageTestSession {
    fn drop(&mut self) {
        if !self.retired {
            let _ = self.cleanup();
        }
    }
}
