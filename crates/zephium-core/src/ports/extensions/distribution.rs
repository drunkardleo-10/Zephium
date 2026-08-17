use crate::extensions::ExtensionCatalogSetDigest;

/// Stable stage of a product extension-distribution failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionDistributionFailureStage {
    Catalog,
    PackageFetch(u8),
    PackageProvision(u8),
    CatalogActivation,
}

/// Redacted, closed reason for a product extension-distribution failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionDistributionFailureReason {
    Acquisition,
    Busy,
    ServiceUnavailable,
    ServiceRejected,
    ServiceFailedClosed,
    SettlementTimedOut,
    SettlementLost,
    SubmissionPanicked,
    OutcomeUnresolved,
    ActivationRejected,
    Accounting,
}

/// Fixed-size successful distribution summary. It carries no package, URL,
/// filesystem, profile, grant, or native-runtime authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionDistributionCompletionStatus {
    catalog_set: ExtensionCatalogSetDigest,
    package_count: u8,
    materialized_packages: u8,
    reused_packages: u8,
    exact_retries: u8,
    newly_activated: bool,
}

impl ExtensionDistributionCompletionStatus {
    pub const fn new(
        catalog_set: ExtensionCatalogSetDigest,
        package_count: u8,
        materialized_packages: u8,
        reused_packages: u8,
        exact_retries: u8,
        newly_activated: bool,
    ) -> Option<Self> {
        let Some(observed) = materialized_packages.checked_add(reused_packages) else {
            return None;
        };
        if package_count == 0 || observed != package_count {
            return None;
        }
        Some(Self {
            catalog_set,
            package_count,
            materialized_packages,
            reused_packages,
            exact_retries,
            newly_activated,
        })
    }

    pub const fn catalog_set(self) -> ExtensionCatalogSetDigest {
        self.catalog_set
    }

    pub const fn package_count(self) -> u8 {
        self.package_count
    }

    pub const fn materialized_packages(self) -> u8 {
        self.materialized_packages
    }

    pub const fn reused_packages(self) -> u8 {
        self.reused_packages
    }

    pub const fn exact_retries(self) -> u8 {
        self.exact_retries
    }

    pub const fn newly_activated(self) -> bool {
        self.newly_activated
    }
}

/// Process-local state of the explicitly constructed product distribution
/// worker. Ordinary builds never construct or publish this state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionDistributionState {
    Idle,
    Synchronizing,
    Ready(ExtensionDistributionCompletionStatus),
    Failed {
        stage: ExtensionDistributionFailureStage,
        reason: ExtensionDistributionFailureReason,
    },
    Quarantined {
        stage: ExtensionDistributionFailureStage,
        reason: ExtensionDistributionFailureReason,
    },
    Shutdown,
}

/// Monotonic status replacement published by one distribution-worker
/// incarnation. A generation of zero is never valid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionDistributionStatus {
    generation: u64,
    state: ExtensionDistributionState,
}

impl ExtensionDistributionStatus {
    pub const fn new(generation: u64, state: ExtensionDistributionState) -> Option<Self> {
        if generation == 0 {
            return None;
        }
        Some(Self { generation, state })
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn state(self) -> ExtensionDistributionState {
        self.state
    }
}

const _: () = assert!(std::mem::size_of::<ExtensionDistributionStatus>() <= 80);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requires_an_exact_nonempty_package_partition() {
        let digest = ExtensionCatalogSetDigest::from_bytes([1; 32]);
        assert!(ExtensionDistributionCompletionStatus::new(digest, 2, 1, 1, 0, true).is_some());
        assert!(ExtensionDistributionCompletionStatus::new(digest, 0, 0, 0, 0, false).is_none());
        assert!(ExtensionDistributionCompletionStatus::new(digest, 2, 2, 1, 0, true).is_none());
    }

    #[test]
    fn status_rejects_zero_generation() {
        assert!(ExtensionDistributionStatus::new(0, ExtensionDistributionState::Idle).is_none());
        assert_eq!(
            ExtensionDistributionStatus::new(1, ExtensionDistributionState::Idle)
                .unwrap()
                .generation(),
            1
        );
    }
}
