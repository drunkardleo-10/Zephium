//! Explicit profile-selection leases for agent-browser contexts.
//!
//! The application actor owns this functional core. It coordinates context
//! lifetime with profile deletion and shutdown without storing profile paths,
//! browser data, extension state, native handles, or background work.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroU64;

use thiserror::Error;
use zephium_core::ids::ProfileId;

use crate::{ContextId, ContextIdentity, ContextKind, MAX_LIVE_CONTEXTS};

/// Maximum number of profile tombstones retained by the process.
pub const MAX_CONTEXT_PROFILE_TOMBSTONES: usize = zephium_core::session::MAX_SESSION_PROFILES;

/// Nonzero process-local identity for one exact profile lease.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContextProfileLeaseId(NonZeroU64);

impl ContextProfileLeaseId {
    /// Constructs a shell-minted nonzero lease identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local correlation value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for ContextProfileLeaseId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ContextProfileLeaseId([redacted])")
    }
}

/// Why one context retains its exact selected profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextProfileLeasePurpose {
    /// Run-owned context using selected-profile storage or its fixed native
    /// automation subprofile.
    Owned,
    /// Existing normal tab whose selected profile already owns its storage.
    BorrowedTab,
    /// Temporary normal context used only for explicit human sign-in.
    HumanSignInHandoff,
}

impl ContextProfileLeasePurpose {
    /// Immutable context kind compatible with the lease purpose.
    pub const fn kind(self) -> ContextKind {
        match self {
            Self::Owned => ContextKind::Owned,
            Self::BorrowedTab => ContextKind::BorrowedTab,
            Self::HumanSignInHandoff => ContextKind::HumanSignInHandoff,
        }
    }
}

/// Copyable authority proving one context retained one explicit profile.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ContextProfileLease {
    id: ContextProfileLeaseId,
    identity: ContextIdentity,
    purpose: ContextProfileLeasePurpose,
}

impl ContextProfileLease {
    /// Process-local exact lease identity.
    pub const fn id(self) -> ContextProfileLeaseId {
        self.id
    }

    /// Complete context/run/profile/kind identity protected by the lease.
    pub const fn identity(self) -> ContextIdentity {
        self.identity
    }

    /// Exact profile-retention purpose.
    pub const fn purpose(self) -> ContextProfileLeasePurpose {
        self.purpose
    }
}

impl fmt::Debug for ContextProfileLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextProfileLease")
            .field("id", &self.id)
            .field("identity", &self.identity)
            .field("purpose", &self.purpose)
            .finish()
    }
}

/// Privacy-preserving profile-lease counts for diagnostics and shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextProfileLeaseStatus {
    active: u8,
    tombstoned_profiles: u8,
    shutdown_sealed: bool,
}

impl ContextProfileLeaseStatus {
    /// Exact number of active context/profile leases.
    pub const fn active(self) -> u8 {
        self.active
    }

    /// Exact number of profiles permanently closed to new agent leases.
    pub const fn tombstoned_profiles(self) -> u8 {
        self.tombstoned_profiles
    }

    /// Whether process shutdown permanently rejected all new leases.
    pub const fn shutdown_sealed(self) -> bool {
        self.shutdown_sealed
    }
}

/// Typed profile-lease refusal without profile names, ids, or paths.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContextProfileLeaseError {
    /// Process shutdown permanently sealed new profile leases.
    #[error("context profile leases are sealed for shutdown")]
    ShutdownSealed,
    /// Profile deletion permanently sealed this exact profile.
    #[error("selected context profile is retired")]
    ProfileRetired,
    /// The fixed active context/profile lease ceiling is full.
    #[error("context profile lease ceiling exceeded")]
    LeaseLimit,
    /// The fixed profile tombstone ceiling is full.
    #[error("context profile tombstone ceiling exceeded")]
    TombstoneLimit,
    /// This context already owns a profile lease.
    #[error("context already has a profile lease")]
    DuplicateContext,
    /// The process-local lease identity is already active.
    #[error("context profile lease identity is already active")]
    DuplicateLease,
    /// Lease purpose does not match immutable context kind.
    #[error("context profile lease purpose is incompatible")]
    PurposeMismatch,
    /// No lease exists for the exact context identity.
    #[error("context profile lease was not found")]
    NotFound,
    /// Release authority does not match the retained lease exactly.
    #[error("context profile lease authority is stale")]
    StaleLease,
    /// Internal bounded accounting became contradictory.
    #[error("context profile lease accounting invariant failed")]
    Invariant,
}

/// Single-owner bounded registry for explicit context/profile leases.
#[derive(Default)]
pub struct ContextProfileLeaseRegistry {
    leases: BTreeMap<ContextId, ContextProfileLease>,
    tombstones: BTreeSet<ProfileId>,
    shutdown_sealed: bool,
}

impl ContextProfileLeaseRegistry {
    /// Creates an empty registry with no worker, timer, native object, or I/O.
    pub const fn new() -> Self {
        Self {
            leases: BTreeMap::new(),
            tombstones: BTreeSet::new(),
            shutdown_sealed: false,
        }
    }

    /// Acquires one lease for the exact profile already selected in `identity`.
    ///
    /// The shell must first resolve that profile from its authoritative profile
    /// registry. This method never substitutes a default profile.
    pub fn acquire(
        &mut self,
        id: ContextProfileLeaseId,
        identity: ContextIdentity,
        purpose: ContextProfileLeasePurpose,
    ) -> Result<ContextProfileLease, ContextProfileLeaseError> {
        self.validate()?;
        if self.shutdown_sealed {
            return Err(ContextProfileLeaseError::ShutdownSealed);
        }
        if self.tombstones.contains(&identity.profile()) {
            return Err(ContextProfileLeaseError::ProfileRetired);
        }
        if purpose.kind() != identity.kind() {
            return Err(ContextProfileLeaseError::PurposeMismatch);
        }
        if self.leases.contains_key(&identity.id()) {
            return Err(ContextProfileLeaseError::DuplicateContext);
        }
        if self.leases.values().any(|lease| lease.id == id) {
            return Err(ContextProfileLeaseError::DuplicateLease);
        }
        if self.leases.len() >= MAX_LIVE_CONTEXTS {
            return Err(ContextProfileLeaseError::LeaseLimit);
        }
        let lease = ContextProfileLease {
            id,
            identity,
            purpose,
        };
        self.leases.insert(identity.id(), lease);
        self.validate()?;
        Ok(lease)
    }

    /// Releases exactly one retained lease without changing context identity.
    pub fn release(
        &mut self,
        lease: ContextProfileLease,
    ) -> Result<ContextIdentity, ContextProfileLeaseError> {
        self.validate()?;
        let retained = self
            .leases
            .get(&lease.identity.id())
            .copied()
            .ok_or(ContextProfileLeaseError::NotFound)?;
        if retained != lease {
            return Err(ContextProfileLeaseError::StaleLease);
        }
        let removed = self
            .leases
            .remove(&lease.identity.id())
            .ok_or(ContextProfileLeaseError::Invariant)?;
        self.validate()?;
        Ok(removed.identity)
    }

    /// Permanently rejects new leases for a profile before deletion begins.
    ///
    /// Existing leases remain live and are returned in stable context order so
    /// the shell can cancel, close/release, and wait before native erasure.
    pub fn tombstone_profile(
        &mut self,
        profile: ProfileId,
    ) -> Result<Vec<ContextProfileLease>, ContextProfileLeaseError> {
        self.validate()?;
        if !self.tombstones.contains(&profile)
            && self.tombstones.len() >= MAX_CONTEXT_PROFILE_TOMBSTONES
        {
            return Err(ContextProfileLeaseError::TombstoneLimit);
        }
        self.tombstones.insert(profile);
        let leases = self.leases_for_profile(profile);
        self.validate()?;
        Ok(leases)
    }

    /// Permanently rejects every new profile lease during process shutdown.
    ///
    /// Existing leases remain live until their exact native resources settle.
    pub fn seal_for_shutdown(
        &mut self,
    ) -> Result<Vec<ContextProfileLease>, ContextProfileLeaseError> {
        self.validate()?;
        self.shutdown_sealed = true;
        let leases = self.leases.values().copied().collect();
        self.validate()?;
        Ok(leases)
    }

    /// Returns the exact lease for one context, if active.
    pub fn lease(&self, context: ContextId) -> Option<ContextProfileLease> {
        self.leases.get(&context).copied()
    }

    /// Returns active leases for one exact profile in stable context order.
    pub fn leases_for_profile(&self, profile: ProfileId) -> Vec<ContextProfileLease> {
        self.leases
            .values()
            .filter(|lease| lease.identity.profile() == profile)
            .copied()
            .collect()
    }

    /// Reports whether profile deletion has permanently sealed new leases.
    pub fn is_profile_tombstoned(&self, profile: ProfileId) -> bool {
        self.tombstones.contains(&profile)
    }

    /// Returns bounded counts without profile identity or native state.
    pub fn status(&self) -> ContextProfileLeaseStatus {
        ContextProfileLeaseStatus {
            active: u8::try_from(self.leases.len()).unwrap_or(u8::MAX),
            tombstoned_profiles: u8::try_from(self.tombstones.len()).unwrap_or(u8::MAX),
            shutdown_sealed: self.shutdown_sealed,
        }
    }

    /// True only after shutdown seal and exact release of every lease.
    pub fn is_quiescent(&self) -> bool {
        self.shutdown_sealed && self.leases.is_empty()
    }

    fn validate(&self) -> Result<(), ContextProfileLeaseError> {
        if self.leases.len() > MAX_LIVE_CONTEXTS {
            return Err(ContextProfileLeaseError::LeaseLimit);
        }
        if self.tombstones.len() > MAX_CONTEXT_PROFILE_TOMBSTONES {
            return Err(ContextProfileLeaseError::TombstoneLimit);
        }
        for (context, lease) in &self.leases {
            if *context != lease.identity.id() || lease.purpose.kind() != lease.identity.kind() {
                return Err(ContextProfileLeaseError::Invariant);
            }
        }
        for (index, lease) in self.leases.values().enumerate() {
            if self
                .leases
                .values()
                .skip(index + 1)
                .any(|candidate| candidate.id == lease.id)
            {
                return Err(ContextProfileLeaseError::Invariant);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextId, ContextRunId};

    fn identity(context: u128, profile: u128, kind: ContextKind) -> ContextIdentity {
        ContextIdentity::new(
            ContextId::from_raw(context),
            ContextRunId::from_raw(70),
            ProfileId::from(profile),
            kind,
        )
    }

    fn lease_id(value: u64) -> ContextProfileLeaseId {
        ContextProfileLeaseId::new(value).expect("lease id")
    }

    #[test]
    fn selection_is_explicit_kind_exact_and_debug_redacted() {
        let mut registry = ContextProfileLeaseRegistry::new();
        let owned = identity(1, 100, ContextKind::Owned);
        assert_eq!(
            registry.acquire(lease_id(1), owned, ContextProfileLeasePurpose::BorrowedTab,),
            Err(ContextProfileLeaseError::PurposeMismatch)
        );
        let lease = registry
            .acquire(lease_id(1), owned, ContextProfileLeasePurpose::Owned)
            .expect("lease");
        assert_eq!(lease.identity(), owned);
        let debug = format!("{lease:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("100"));
    }

    #[test]
    fn duplicate_context_and_lease_ids_fail_without_replacement() {
        let mut registry = ContextProfileLeaseRegistry::new();
        let first = registry
            .acquire(
                lease_id(1),
                identity(1, 100, ContextKind::Owned),
                ContextProfileLeasePurpose::Owned,
            )
            .expect("first");
        assert_eq!(
            registry.acquire(
                lease_id(2),
                identity(1, 100, ContextKind::Owned),
                ContextProfileLeasePurpose::Owned,
            ),
            Err(ContextProfileLeaseError::DuplicateContext)
        );
        assert_eq!(
            registry.acquire(
                lease_id(1),
                identity(2, 100, ContextKind::Owned),
                ContextProfileLeasePurpose::Owned,
            ),
            Err(ContextProfileLeaseError::DuplicateLease)
        );
        assert_eq!(registry.lease(first.identity().id()), Some(first));
    }

    #[test]
    fn profile_tombstone_is_a_race_free_deletion_barrier() {
        let mut registry = ContextProfileLeaseRegistry::new();
        let first = registry
            .acquire(
                lease_id(1),
                identity(1, 100, ContextKind::Owned),
                ContextProfileLeasePurpose::Owned,
            )
            .expect("first");
        let second = registry
            .acquire(
                lease_id(2),
                identity(2, 200, ContextKind::BorrowedTab),
                ContextProfileLeasePurpose::BorrowedTab,
            )
            .expect("second");
        assert_eq!(
            registry
                .tombstone_profile(ProfileId::from(100))
                .expect("tombstone"),
            vec![first]
        );
        assert_eq!(
            registry.acquire(
                lease_id(3),
                identity(3, 100, ContextKind::Owned),
                ContextProfileLeasePurpose::Owned,
            ),
            Err(ContextProfileLeaseError::ProfileRetired)
        );
        registry.release(first).expect("release first");
        assert!(registry.leases_for_profile(ProfileId::from(100)).is_empty());
        assert_eq!(registry.lease(second.identity().id()), Some(second));
    }

    #[test]
    fn stale_release_cannot_drop_an_active_lease() {
        let mut registry = ContextProfileLeaseRegistry::new();
        let retained = registry
            .acquire(
                lease_id(1),
                identity(1, 100, ContextKind::Owned),
                ContextProfileLeasePurpose::Owned,
            )
            .expect("lease");
        let stale = ContextProfileLease {
            id: lease_id(2),
            ..retained
        };
        assert_eq!(
            registry.release(stale),
            Err(ContextProfileLeaseError::StaleLease)
        );
        assert_eq!(registry.lease(retained.identity().id()), Some(retained));
    }

    #[test]
    fn lease_capacity_refuses_without_eviction() {
        let mut registry = ContextProfileLeaseRegistry::new();
        for value in 1..=MAX_LIVE_CONTEXTS {
            registry
                .acquire(
                    lease_id(u64::try_from(value).expect("small value")),
                    identity(value as u128, 100, ContextKind::Owned),
                    ContextProfileLeasePurpose::Owned,
                )
                .expect("within limit");
        }
        assert_eq!(
            registry.acquire(
                lease_id(99),
                identity(99, 100, ContextKind::Owned),
                ContextProfileLeasePurpose::Owned,
            ),
            Err(ContextProfileLeaseError::LeaseLimit)
        );
        assert_eq!(registry.status().active(), MAX_LIVE_CONTEXTS as u8);
    }

    #[test]
    fn shutdown_seal_retains_cleanup_authority_until_exact_release() {
        let mut registry = ContextProfileLeaseRegistry::new();
        let lease = registry
            .acquire(
                lease_id(1),
                identity(1, 100, ContextKind::HumanSignInHandoff),
                ContextProfileLeasePurpose::HumanSignInHandoff,
            )
            .expect("lease");
        assert_eq!(registry.seal_for_shutdown().expect("seal"), vec![lease]);
        assert!(!registry.is_quiescent());
        assert_eq!(
            registry.acquire(
                lease_id(2),
                identity(2, 100, ContextKind::Owned),
                ContextProfileLeasePurpose::Owned,
            ),
            Err(ContextProfileLeaseError::ShutdownSealed)
        );
        registry.release(lease).expect("release");
        assert!(registry.is_quiescent());
    }
}
