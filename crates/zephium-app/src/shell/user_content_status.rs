//! Bounded authoritative projection of native user-content settlement.

use std::collections::HashMap;

use zephium_core::ports::engine::{ContentScope, UserContentGeneration, UserContentSettlement};

const MAX_USER_CONTENT_SCOPES: usize = zephium_core::session::MAX_SESSION_PROFILES + 1;

#[derive(Clone, Copy)]
struct ScopeStatus {
    requested: UserContentGeneration,
    degraded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UserContentObservation {
    Changed,
    Unchanged,
    Stale,
    Contradictory,
    CapacityExceeded,
}

/// Latest terminal generation per global/profile ownership scope.
///
/// Successful entries are retained as monotonic fences so an older failure
/// cannot resurrect a warning after mailbox coalescing. The scope cohort is
/// bounded by the session profile ceiling plus the one process-global scope.
pub(super) struct UserContentStatus {
    scopes: HashMap<ContentScope, ScopeStatus>,
    overflowed: bool,
}

impl Default for UserContentStatus {
    fn default() -> Self {
        Self {
            scopes: HashMap::with_capacity(MAX_USER_CONTENT_SCOPES),
            overflowed: false,
        }
    }
}

impl UserContentStatus {
    pub(super) fn observe(
        &mut self,
        scope: ContentScope,
        requested: UserContentGeneration,
        settlement: &UserContentSettlement,
    ) -> UserContentObservation {
        let degraded = !matches!(
            settlement,
            UserContentSettlement::Applied { generation } if *generation == requested
        );

        if let Some(current) = self.scopes.get_mut(&scope) {
            if requested < current.requested {
                return UserContentObservation::Stale;
            }
            if requested == current.requested {
                if current.degraded == degraded {
                    return UserContentObservation::Unchanged;
                }
                // A generation has exactly one terminal native truth. Preserve
                // the conservative result if contradictory callbacks arrive.
                current.degraded = true;
                return UserContentObservation::Contradictory;
            }
            *current = ScopeStatus {
                requested,
                degraded,
            };
            return UserContentObservation::Changed;
        }

        if self.scopes.len() >= MAX_USER_CONTENT_SCOPES {
            let changed = !self.overflowed;
            self.overflowed = true;
            return if changed {
                UserContentObservation::CapacityExceeded
            } else {
                UserContentObservation::Unchanged
            };
        }
        self.scopes.insert(
            scope,
            ScopeStatus {
                requested,
                degraded,
            },
        );
        UserContentObservation::Changed
    }

    pub(super) fn retire_profile(&mut self, profile: zephium_core::ids::ProfileId) -> bool {
        self.scopes
            .remove(&ContentScope::Profile(profile))
            .is_some_and(|status| status.degraded)
    }

    pub(super) fn degraded_scope_count(&self) -> u16 {
        let degraded = self
            .scopes
            .values()
            .filter(|status| status.degraded)
            .count()
            .saturating_add(usize::from(self.overflowed));
        // Overflow is an invariant warning, not another ownership scope. Keep
        // the privacy-preserving aggregate inside the same public scope bound.
        u16::try_from(degraded.min(MAX_USER_CONTENT_SCOPES)).unwrap_or(u16::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_core::ports::engine::UserContentApplyFailure;

    fn generation(value: u64) -> UserContentGeneration {
        UserContentGeneration::new(value).expect("nonzero generation")
    }

    #[test]
    fn newer_success_clears_failure_and_stale_failure_cannot_restore_it() {
        let mut status = UserContentStatus::default();
        assert_eq!(
            status.observe(
                ContentScope::Global,
                generation(1),
                &UserContentSettlement::Unavailable {
                    failure: UserContentApplyFailure::NativeInstallation,
                },
            ),
            UserContentObservation::Changed
        );
        assert_eq!(status.degraded_scope_count(), 1);
        assert_eq!(
            status.observe(
                ContentScope::Global,
                generation(2),
                &UserContentSettlement::Applied {
                    generation: generation(2),
                },
            ),
            UserContentObservation::Changed
        );
        assert_eq!(status.degraded_scope_count(), 0);
        assert_eq!(
            status.observe(
                ContentScope::Global,
                generation(1),
                &UserContentSettlement::Unavailable {
                    failure: UserContentApplyFailure::NativeCleanup,
                },
            ),
            UserContentObservation::Stale
        );
        assert_eq!(status.degraded_scope_count(), 0);
    }

    #[test]
    fn contradictory_terminal_for_one_generation_fails_closed() {
        let mut status = UserContentStatus::default();
        assert_eq!(
            status.observe(
                ContentScope::Global,
                generation(7),
                &UserContentSettlement::Applied {
                    generation: generation(7),
                },
            ),
            UserContentObservation::Changed
        );
        assert_eq!(
            status.observe(
                ContentScope::Global,
                generation(7),
                &UserContentSettlement::Unavailable {
                    failure: UserContentApplyFailure::UnsupportedPlatform,
                },
            ),
            UserContentObservation::Contradictory
        );
        assert_eq!(status.degraded_scope_count(), 1);

        let mut reverse = UserContentStatus::default();
        reverse.observe(
            ContentScope::Global,
            generation(7),
            &UserContentSettlement::Unavailable {
                failure: UserContentApplyFailure::NativeInstallation,
            },
        );
        assert_eq!(
            reverse.observe(
                ContentScope::Global,
                generation(7),
                &UserContentSettlement::Applied {
                    generation: generation(7),
                },
            ),
            UserContentObservation::Contradictory
        );
        assert_eq!(reverse.degraded_scope_count(), 1);
    }

    #[test]
    fn profile_retirement_removes_only_that_profiles_projection() {
        let mut status = UserContentStatus::default();
        let unavailable = UserContentSettlement::Unavailable {
            failure: UserContentApplyFailure::NativeInstallation,
        };
        let applied = UserContentSettlement::Applied {
            generation: generation(1),
        };
        let healthy = zephium_core::ids::ProfileId::from(17);
        let degraded = zephium_core::ids::ProfileId::from(18);

        status.observe(ContentScope::Global, generation(1), &unavailable);
        status.observe(ContentScope::Profile(healthy), generation(1), &applied);
        status.observe(ContentScope::Profile(degraded), generation(1), &unavailable);
        assert_eq!(status.degraded_scope_count(), 2);

        assert!(!status.retire_profile(healthy));
        assert_eq!(status.degraded_scope_count(), 2);
        assert!(status.retire_profile(degraded));
        assert_eq!(status.degraded_scope_count(), 1);
        assert!(!status.retire_profile(degraded));
    }

    #[test]
    fn scope_capacity_overflow_is_bounded_and_sticky() {
        let mut status = UserContentStatus::default();
        let applied = UserContentSettlement::Applied {
            generation: generation(1),
        };
        assert_eq!(
            status.observe(ContentScope::Global, generation(1), &applied),
            UserContentObservation::Changed
        );
        for raw in 1..=zephium_core::session::MAX_SESSION_PROFILES {
            assert_eq!(
                status.observe(
                    ContentScope::Profile(zephium_core::ids::ProfileId::from(raw as u128)),
                    generation(1),
                    &applied,
                ),
                UserContentObservation::Changed
            );
        }
        assert_eq!(status.degraded_scope_count(), 0);
        assert_eq!(
            status.observe(
                ContentScope::Profile(zephium_core::ids::ProfileId::from(u128::MAX)),
                generation(1),
                &applied,
            ),
            UserContentObservation::CapacityExceeded
        );
        assert_eq!(status.degraded_scope_count(), 1);
        assert!(!status.retire_profile(zephium_core::ids::ProfileId::from(1)));
        assert_eq!(status.degraded_scope_count(), 1);

        let unavailable = UserContentSettlement::Unavailable {
            failure: UserContentApplyFailure::NativeInstallation,
        };
        assert_eq!(
            status.observe(ContentScope::Global, generation(2), &unavailable),
            UserContentObservation::Changed
        );
        for raw in 1..=zephium_core::session::MAX_SESSION_PROFILES {
            assert_eq!(
                status.observe(
                    ContentScope::Profile(zephium_core::ids::ProfileId::from(raw as u128)),
                    generation(2),
                    &unavailable,
                ),
                UserContentObservation::Changed
            );
        }
        assert_eq!(
            status.degraded_scope_count(),
            u16::try_from(MAX_USER_CONTENT_SCOPES).unwrap()
        );
    }
}
