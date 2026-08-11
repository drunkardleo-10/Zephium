use std::time::Instant;

use crate::extensions::{
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionNativeOwnershipKey,
    ExtensionRuntimeGeneration,
};
use crate::ids::{ExtensionInstallId, ProfileId};

/// The extension runtime pool has three background slots, so startup can
/// expose at most three distinct profiles with executable runtime authority.
/// Keep this projection fixed-size and allocation-free.
pub const MAX_EXTENSION_ACTIVE_PROFILES: usize = 3;

/// Exact profiles whose extension runtimes were active when startup settled.
///
/// This value is routing data, not Store, package, grant, or native-owner
/// authority. It exists solely so the Shell can publish logical browser tabs
/// to the already-authorized native controller without broadcasting every
/// profile or waking the inert extension path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtensionActiveProfiles {
    entries: [[u8; 16]; MAX_EXTENSION_ACTIVE_PROFILES],
    length: u8,
}

const _: () = assert!(std::mem::size_of::<ExtensionActiveProfiles>() <= 64);

impl ExtensionActiveProfiles {
    pub const EMPTY: Self = Self {
        entries: [[0; 16]; MAX_EXTENSION_ACTIVE_PROFILES],
        length: 0,
    };

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub const fn len(&self) -> usize {
        self.length as usize
    }

    pub fn contains(&self, profile: ProfileId) -> bool {
        self.entries[..self.len()].contains(&profile.bytes())
    }

    /// Inserts one unique profile without allocation. Re-inserting an exact
    /// identity succeeds; exceeding the shared runtime ceiling is refused.
    pub fn try_insert(&mut self, profile: ProfileId) -> bool {
        if self.contains(profile) {
            return true;
        }
        let index = self.len();
        if index == MAX_EXTENSION_ACTIVE_PROFILES {
            return false;
        }
        self.entries[index] = profile.bytes();
        self.length += 1;
        true
    }

    pub fn remove(&mut self, profile: ProfileId) -> bool {
        let length = self.len();
        let Some(index) = self.entries[..length]
            .iter()
            .position(|entry| *entry == profile.bytes())
        else {
            return false;
        };
        self.entries.copy_within(index + 1..length, index);
        self.entries[length - 1] = [0; 16];
        self.length -= 1;
        true
    }

    pub fn iter(self) -> impl Iterator<Item = ProfileId> {
        self.entries
            .into_iter()
            .take(self.len())
            .map(|bytes| ProfileId::from(u128::from_be_bytes(bytes)))
    }
}

/// Application-facing result of consuming the extension-service owner.
///
/// `Unclean` is terminal. The concrete service may retain more precise
/// diagnostics, but once its unique owner has been consumed there is no safe
/// in-process retry surface for application code.
#[must_use = "extension-service shutdown must be checked before Store teardown"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceShutdownOutcome {
    /// The service proved that its worker terminated and released its owned
    /// resources after draining every admitted command.
    Clean,
    /// The service could not prove clean worker termination and resource
    /// release before returning.
    Unclean,
}

/// Application-facing settlement of extension-service startup.
///
/// This projection intentionally carries no package, repository, grant, or
/// native-owner identity. `Ready` includes only the fixed, non-authoritative
/// profile set needed for Shell-owned browser routing. Only `Ready` authorizes
/// callers to continue into extension-sensitive bootstrap work such as
/// recovered profile deletion or raw content-view construction.
#[must_use = "extension-sensitive bootstrap requires an explicit Ready settlement"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStartupOutcome {
    /// Repository recovery and every durable possible-owner cleanup completed.
    Ready(ExtensionActiveProfiles),
    /// One or more possible native owners remain and require cleanup.
    CleanupRequired,
    /// The exact startup attempt settled with a definite retryable condition.
    Unavailable,
    /// Startup detected corruption, invariant loss, or another closed failure.
    FailedClosed,
    /// The caller's observation deadline elapsed without terminal settlement.
    TimedOut,
    /// A retry was definitely not admitted because bounded transient capacity
    /// was unavailable. No startup frontier ran, so a later bounded retry is
    /// permitted while the same owner remains live.
    RetryableNotAdmitted,
}

/// Coarse settlement of one synchronous profile-retirement continuation.
///
/// This value is deliberately informational and publicly constructible. It is
/// not an unforgeable capability. Zephium's audited composition rule is that
/// Store authorization, native website-data erasure, and Store finalization
/// occur only inside the continuation passed to
/// [`ExtensionServiceLifecycle::with_profile_retired_until`]. The service
/// invokes that continuation only after its worker directly settles the exact
/// profile as retired. App-level behavior tests protect all three call sites;
/// the Rust type system does not independently enforce that architectural
/// rule.
#[must_use = "profile-retirement continuation settlement must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionProfileRetirementDisposition {
    /// The service proved retirement and invoked the continuation exactly
    /// once before returning.
    Continued,
    /// Retirement did not settle in this attempt. The continuation was not
    /// invoked and profile data must remain intact for a bounded retry.
    Unavailable,
    /// The service failed closed. The continuation was not invoked and the
    /// current process must not continue profile deletion.
    FailedClosed,
}

/// Application-facing settlement of one exact runtime activation request.
///
/// The ownership key is only a selector. The concrete service reconstructs
/// Store, package, grant, repository, and native authority inside its
/// serialized worker. Successful variants carry the complete non-authorizing
/// profile routing snapshot observed in the same worker turn.
#[must_use = "runtime activation settlement must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeActivationDisposition {
    Activated {
        generation: ExtensionRuntimeGeneration,
        active_profiles: ExtensionActiveProfiles,
    },
    AlreadyActive {
        generation: ExtensionRuntimeGeneration,
        active_profiles: ExtensionActiveProfiles,
    },
    Unavailable,
    Rejected,
    CapacityExceeded,
    ProfileFenced,
    FailedClosed,
}

/// Application-facing settlement of one exact runtime retirement request.
///
/// `NotPresent` is process-local coordinator evidence only. It does not prove
/// durable profile absence and must never authorize profile deletion. Both
/// successful variants carry the complete routing snapshot from the same
/// serialized worker turn, so Shell never guesses whether a sibling runtime
/// still keeps the profile active.
#[must_use = "runtime retirement settlement must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeRetirementDisposition {
    Retired {
        active_profiles: ExtensionActiveProfiles,
    },
    NotPresent {
        active_profiles: ExtensionActiveProfiles,
    },
    Unavailable,
    FailedClosed,
}

/// Stale-UI-resistant selector for one installed extension.
///
/// The revisions are compare-and-swap inputs, not Store or package authority.
/// Shell constructs this value only from its latest privileged management
/// projection; the serialized service reloads and revalidates the complete
/// catalog before changing runtime or durable state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionInstallSelector {
    profile: ProfileId,
    install: ExtensionInstallId,
    catalog_revision: ExtensionInstallCatalogRevision,
    install_revision: ExtensionInstallRevision,
}

impl ExtensionInstallSelector {
    pub const fn new(
        profile: ProfileId,
        install: ExtensionInstallId,
        catalog_revision: ExtensionInstallCatalogRevision,
        install_revision: ExtensionInstallRevision,
    ) -> Self {
        Self {
            profile,
            install,
            catalog_revision,
            install_revision,
        }
    }

    pub const fn profile(self) -> ProfileId {
        self.profile
    }

    pub const fn install(self) -> ExtensionInstallId {
        self.install
    }

    pub const fn catalog_revision(self) -> ExtensionInstallCatalogRevision {
        self.catalog_revision
    }

    pub const fn install_revision(self) -> ExtensionInstallRevision {
        self.install_revision
    }
}

/// Why enabled user intent was durably accepted but the regular runtime is
/// not active in the current process.
///
/// This is a truthful state, not a successful enablement claim. Restart or a
/// later explicit retry re-enters the ordinary authenticated activation path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionActivationPendingReason {
    Unavailable,
    Rejected,
    CapacityExceeded,
    ProfileFenced,
    FailedClosed,
}

/// Exact result of one serialized enable/disable transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionSetEnabledOutcome {
    Enabled {
        generation: ExtensionRuntimeGeneration,
        changed: bool,
    },
    Disabled {
        changed: bool,
    },
    /// Durable desired-enabled state is true, but activation did not settle.
    PendingActivation(ExtensionActivationPendingReason),
    /// The privileged projection was stale. No durable mutation was admitted.
    Conflict,
    /// Current package/grant state cannot authorize the requested transition.
    Rejected,
    Unavailable,
    /// A Store mutation was admitted but its commit could not be observed.
    /// The process must reconcile before accepting another management write.
    OutcomeUnknown,
    FailedClosed,
}

/// Exact result of one serialized uninstall transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionUninstallOutcome {
    Uninstalled,
    Conflict,
    Rejected,
    Unavailable,
    OutcomeUnknown,
    FailedClosed,
}

/// Management settlement plus the complete same-turn browser-routing cohort.
///
/// `None` means the worker could not establish a trustworthy runtime
/// projection. Shell must retain its previous projection in that case.
#[must_use = "extension management settlement and routing projection must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionManagementSettlement<T> {
    outcome: T,
    active_profiles: Option<ExtensionActiveProfiles>,
}

/// Non-blocking admission result for a management transaction.
///
/// `Accepted` transfers exactly-once callback ownership to the service. Every
/// refusal leaves durable and native state untouched and drops the callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementAdmission {
    Accepted,
    Busy,
    Unavailable,
}

/// Exactly-once completion callback for an admitted enable/disable request.
pub type ExtensionSetEnabledCallback =
    Box<dyn FnOnce(ExtensionManagementSettlement<ExtensionSetEnabledOutcome>) + Send>;
/// Exactly-once completion callback for an admitted uninstall request.
pub type ExtensionUninstallCallback =
    Box<dyn FnOnce(ExtensionManagementSettlement<ExtensionUninstallOutcome>) + Send>;

impl<T> ExtensionManagementSettlement<T> {
    pub const fn new(outcome: T, active_profiles: Option<ExtensionActiveProfiles>) -> Self {
        Self {
            outcome,
            active_profiles,
        }
    }

    pub const fn outcome(&self) -> &T {
        &self.outcome
    }

    pub const fn active_profiles(&self) -> Option<ExtensionActiveProfiles> {
        self.active_profiles
    }

    pub fn into_outcome(self) -> T {
        self.outcome
    }
}

/// Move-only application lifecycle boundary for the extension service.
///
/// The unique owner is held behind a `Box` and consumed by shutdown. This
/// keeps implementation-specific mutation and cleanup authority out of the
/// application layer, while allowing the owner to move onto the application
/// actor thread. No cloneable or borrowed shutdown operation exists.
pub trait ExtensionServiceLifecycle: Send {
    /// Observes the active startup attempt, or admits one bounded retry when
    /// the previous exact attempt settled unavailable.
    ///
    /// This call may wait until the absolute `deadline` and must run away from
    /// a native UI/event-loop thread: native-owner reconciliation can require
    /// that same thread to service WebKit/WebView2 work. Only
    /// [`ExtensionServiceStartupOutcome::Ready`] permits extension-sensitive
    /// application bootstrap.
    fn settle_startup_until(&mut self, deadline: Instant) -> ExtensionServiceStartupOutcome;

    /// Activates one exact regular/private runtime through the service's
    /// authenticated, serialized authority transaction.
    ///
    /// This call follows the same native-event-loop restriction as startup.
    /// The default fails closed so inert and test adapters cannot accidentally
    /// claim activation without owning the runtime coordinator.
    fn activate_runtime_until(
        &mut self,
        _key: ExtensionNativeOwnershipKey,
        _deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        ExtensionRuntimeActivationDisposition::FailedClosed
    }

    /// Retires one exact runtime before Shell mutates durable disable or
    /// uninstall intent.
    ///
    /// The returned active-profile projection is routing data only. The
    /// default fails closed and grants no absence authority.
    fn retire_runtime_until(
        &mut self,
        _key: ExtensionNativeOwnershipKey,
        _deadline: Instant,
    ) -> ExtensionRuntimeRetirementDisposition {
        ExtensionRuntimeRetirementDisposition::FailedClosed
    }

    /// Atomically coordinates one stale-resistant enable/disable request with
    /// native runtime ownership and durable desired state.
    ///
    /// Implementations retire both regular and private owners before a
    /// disabling write, and persist enabled intent before entering ordinary
    /// authenticated activation. The default fails closed.
    fn set_install_enabled_until(
        &mut self,
        _selector: ExtensionInstallSelector,
        _enabled: bool,
        _deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionSetEnabledOutcome> {
        ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None)
    }

    /// Non-blocking form used by the application actor. `Accepted` transfers
    /// callback ownership and the implementation must settle it exactly once
    /// after its serialized worker completes the request.
    fn begin_set_install_enabled(
        &mut self,
        _selector: ExtensionInstallSelector,
        _enabled: bool,
        _deadline: Instant,
        done: ExtensionSetEnabledCallback,
    ) -> ExtensionManagementAdmission {
        drop(done);
        ExtensionManagementAdmission::Unavailable
    }

    /// Retires every regular/private owner before deleting one exact install.
    /// Grant rows are subordinate durable state and are removed by the same
    /// Store transaction. The default fails closed.
    fn uninstall_until(
        &mut self,
        _selector: ExtensionInstallSelector,
        _deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionUninstallOutcome> {
        ExtensionManagementSettlement::new(ExtensionUninstallOutcome::FailedClosed, None)
    }

    /// Non-blocking uninstall form used by the application actor.
    fn begin_uninstall(
        &mut self,
        _selector: ExtensionInstallSelector,
        _deadline: Instant,
        done: ExtensionUninstallCallback,
    ) -> ExtensionManagementAdmission {
        drop(done);
        ExtensionManagementAdmission::Unavailable
    }

    /// Permanently fences `profile`, proves every extension-owned durable,
    /// package, and native obligation absent, then invokes `continuation`
    /// exactly once before returning [`ExtensionProfileRetirementDisposition::Continued`].
    ///
    /// The continuation is the authority boundary. Implementations must drop
    /// it without invocation for every unavailable or failed-closed result.
    /// Zephium callers must put Store authorization, native website-data
    /// erasure, or Store finalization *inside* this continuation and must not
    /// branch on the copied return value to perform those effects later. This
    /// is an audited composition rule, not a type-level capability guarantee.
    ///
    /// This call may wait until the absolute `deadline` and follows the same
    /// native-event-loop restriction as [`Self::settle_startup_until`]. The
    /// default is fail-closed so test-only or compatibility lifecycle adapters
    /// cannot accidentally authorize deletion when they have no retirement
    /// implementation.
    fn with_profile_retired_until(
        &mut self,
        _profile: ProfileId,
        _deadline: Instant,
        continuation: Box<dyn FnOnce() + '_>,
    ) -> ExtensionProfileRetirementDisposition {
        drop(continuation);
        ExtensionProfileRetirementDisposition::FailedClosed
    }

    /// Seals service admission and consumes the unique owner while attempting
    /// to prove worker termination and resource release by `deadline`.
    ///
    /// The deadline is absolute. Every non-clean concrete service outcome
    /// must project to [`ExtensionServiceShutdownOutcome::Unclean`]; ownership
    /// has already been consumed and cannot be retried in-process.
    fn shutdown_until(self: Box<Self>, deadline: Instant) -> ExtensionServiceShutdownOutcome;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::*;

    struct TestLifecycle {
        consumed: Arc<AtomicBool>,
    }

    impl ExtensionServiceLifecycle for TestLifecycle {
        fn settle_startup_until(&mut self, _deadline: Instant) -> ExtensionServiceStartupOutcome {
            ExtensionServiceStartupOutcome::Ready(ExtensionActiveProfiles::EMPTY)
        }

        fn with_profile_retired_until(
            &mut self,
            _profile: ProfileId,
            _deadline: Instant,
            continuation: Box<dyn FnOnce() + '_>,
        ) -> ExtensionProfileRetirementDisposition {
            continuation();
            ExtensionProfileRetirementDisposition::Continued
        }

        fn shutdown_until(self: Box<Self>, _deadline: Instant) -> ExtensionServiceShutdownOutcome {
            self.consumed.store(true, Ordering::Release);
            ExtensionServiceShutdownOutcome::Clean
        }
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn lifecycle_is_send_object_safe_and_consumed_by_shutdown() {
        assert_send::<Box<dyn ExtensionServiceLifecycle>>();
        let consumed = Arc::new(AtomicBool::new(false));
        let mut lifecycle: Box<dyn ExtensionServiceLifecycle> = Box::new(TestLifecycle {
            consumed: Arc::clone(&consumed),
        });

        assert_eq!(
            lifecycle.settle_startup_until(Instant::now()),
            ExtensionServiceStartupOutcome::Ready(ExtensionActiveProfiles::EMPTY)
        );
        let key = ExtensionNativeOwnershipKey::new(
            ProfileId::from(7),
            crate::ids::ExtensionInstallId::from(1),
            crate::extensions::ExtensionGrantBrowsingContext::Regular,
        );
        assert_eq!(
            lifecycle.activate_runtime_until(key, Instant::now()),
            ExtensionRuntimeActivationDisposition::FailedClosed
        );
        assert_eq!(
            lifecycle.retire_runtime_until(key, Instant::now()),
            ExtensionRuntimeRetirementDisposition::FailedClosed
        );
        let selector = ExtensionInstallSelector::new(
            ProfileId::from(7),
            crate::ids::ExtensionInstallId::from(1),
            crate::extensions::ExtensionInstallCatalogRevision::INITIAL,
            crate::extensions::ExtensionInstallRevision::INITIAL,
        );
        assert_eq!(
            lifecycle.set_install_enabled_until(selector, true, Instant::now()),
            ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None)
        );
        assert_eq!(
            lifecycle.uninstall_until(selector, Instant::now()),
            ExtensionManagementSettlement::new(ExtensionUninstallOutcome::FailedClosed, None)
        );
        let continued = Arc::new(AtomicBool::new(false));
        let continued_by_callback = Arc::clone(&continued);
        assert_eq!(
            lifecycle.with_profile_retired_until(
                ProfileId::from(7),
                Instant::now(),
                Box::new(move || continued_by_callback.store(true, Ordering::Release)),
            ),
            ExtensionProfileRetirementDisposition::Continued
        );
        assert!(continued.load(Ordering::Acquire));
        assert_eq!(
            lifecycle.shutdown_until(Instant::now()),
            ExtensionServiceShutdownOutcome::Clean
        );
        assert!(consumed.load(Ordering::Acquire));
    }

    #[test]
    fn active_profile_projection_is_unique_compact_and_bounded() {
        let mut profiles = ExtensionActiveProfiles::EMPTY;
        for value in 1..=MAX_EXTENSION_ACTIVE_PROFILES {
            assert!(profiles.try_insert(ProfileId::from(value as u128)));
        }
        assert!(profiles.try_insert(ProfileId::from(2)));
        assert!(!profiles.try_insert(ProfileId::from(99)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(2), ProfileId::from(3)]
        );
        assert!(profiles.remove(ProfileId::from(2)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(3)]
        );
        assert!(!profiles.remove(ProfileId::from(2)));
        assert!(profiles.try_insert(ProfileId::from(4)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(3), ProfileId::from(4)]
        );
    }
}
