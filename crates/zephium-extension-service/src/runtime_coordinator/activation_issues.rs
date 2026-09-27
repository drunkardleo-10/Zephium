//! Fixed-size, process-local failure explanations. Never operation authority.
use super::outcome::RuntimeActivationOutcome;
use zephium_core::extensions::ExtensionNativeOwnershipKey;
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::ExtensionActivationPendingReason as Reason;

const LIMIT: usize = crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES * 2;

pub(super) struct ActivationIssues {
    entries: [Option<(ExtensionNativeOwnershipKey, Reason)>; LIMIT],
    next: usize,
}

impl ActivationIssues {
    pub(super) const fn new() -> Self {
        Self {
            entries: [None; LIMIT],
            next: 0,
        }
    }

    pub(super) fn get(&self, key: ExtensionNativeOwnershipKey) -> Option<Reason> {
        self.entries
            .iter()
            .flatten()
            .find(|(stored, _)| *stored == key)
            .map(|(_, reason)| *reason)
    }

    pub(super) fn clear(&mut self, key: ExtensionNativeOwnershipKey) {
        for entry in &mut self.entries {
            if entry.is_some_and(|(stored, _)| stored == key) {
                *entry = None;
            }
        }
    }

    pub(super) fn clear_profile(&mut self, profile: ProfileId) {
        for entry in &mut self.entries {
            if entry.is_some_and(|(stored, _)| stored.profile() == profile) {
                *entry = None;
            }
        }
    }

    pub(super) fn record(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        outcome: RuntimeActivationOutcome,
    ) {
        self.clear(key);
        let reason = match outcome {
            RuntimeActivationOutcome::Activated(_) | RuntimeActivationOutcome::AlreadyActive(_) => {
                return
            }
            RuntimeActivationOutcome::Unavailable(reason) => reason.pending_reason(),
            RuntimeActivationOutcome::Rejected(_) => Reason::Rejected,
            RuntimeActivationOutcome::CapacityExceeded => Reason::CapacityExceeded,
            RuntimeActivationOutcome::ProfileFenced => Reason::ProfileFenced,
            RuntimeActivationOutcome::FailedClosed(_) => Reason::FailedClosed,
        };
        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.next);
        self.entries[slot] = Some((key, reason));
        self.next = (slot + 1) % LIMIT;
    }
}

#[cfg(test)]
mod tests {
    use super::super::outcome::{
        RuntimeActivationRejectionReason, RuntimeActivationUnavailableReason,
    };
    use super::*;
    use zephium_core::extensions::{ExtensionGrantBrowsingContext, ExtensionRuntimeGeneration};
    use zephium_core::ids::ExtensionInstallId;

    fn key(profile: u128, install: u128) -> ExtensionNativeOwnershipKey {
        ExtensionNativeOwnershipKey::new(
            ProfileId::from(profile),
            ExtensionInstallId::from(install),
            ExtensionGrantBrowsingContext::Regular,
        )
    }

    #[test]
    fn successful_retry_clears_only_the_exact_failed_runtime() {
        let mut issues = ActivationIssues::new();
        let first = key(1, 1);
        let other_profile = key(2, 1);
        for key in [first, other_profile] {
            issues.record(
                key,
                RuntimeActivationOutcome::Rejected(
                    RuntimeActivationRejectionReason::HostUnsupported,
                ),
            );
        }
        issues.record(
            first,
            RuntimeActivationOutcome::Activated(ExtensionRuntimeGeneration::INITIAL),
        );
        assert_eq!(issues.get(first), None);
        assert_eq!(issues.get(other_profile), Some(Reason::Rejected));
        issues.clear_profile(ProfileId::from(2));
        assert_eq!(issues.get(other_profile), None);
    }

    #[test]
    fn bounded_explanations_evict_without_creating_runtime_authority() {
        let mut issues = ActivationIssues::new();
        for install in 0..(LIMIT + 7) {
            issues.record(
                key(1, install as u128),
                RuntimeActivationOutcome::CapacityExceeded,
            );
        }
        assert_eq!(issues.entries.iter().flatten().count(), LIMIT);
        assert_eq!(
            issues.get(key(1, (LIMIT + 6) as u128)),
            Some(Reason::CapacityExceeded)
        );
        assert_eq!(issues.get(key(1, 0)), None);
        issues.clear_profile(ProfileId::from(1));
        assert!(issues.entries.iter().all(Option::is_none));
    }

    #[test]
    fn restart_requirement_survives_projection_until_a_new_attempt() {
        let mut issues = ActivationIssues::new();
        let key = key(1, 4);
        issues.record(
            key,
            RuntimeActivationOutcome::Unavailable(
                RuntimeActivationUnavailableReason::NativeRetryable(
                    zephium_extension_runtime_api::ExtensionRuntimeFailure::RestartRequired,
                ),
            ),
        );
        assert_eq!(issues.get(key), Some(Reason::RestartRequired));
        issues.record(
            key,
            RuntimeActivationOutcome::Unavailable(
                RuntimeActivationUnavailableReason::DeadlineReached,
            ),
        );
        assert_eq!(issues.get(key), Some(Reason::Unavailable));
        issues.record(
            key,
            RuntimeActivationOutcome::AlreadyActive(ExtensionRuntimeGeneration::INITIAL),
        );
        assert_eq!(issues.get(key), None);
    }
}
