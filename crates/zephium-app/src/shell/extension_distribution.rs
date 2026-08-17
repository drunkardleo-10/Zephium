//! Actor-ordered bridge from product distribution into the unique extension
//! service lifecycle.
//!
//! Network acquisition stays on its caller's async worker. Shell receives at
//! most one already bounded, move-only request, performs no parsing or I/O,
//! and never lends its mutable lifecycle owner across an await.

use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationCallback, ExtensionAcquiredCatalogActivationOutcome,
    ExtensionAcquiredPackageProvisioningCallback, ExtensionAcquiredPackageProvisioningOutcome,
    ExtensionManagementAdmission,
};

use super::*;
use crate::api::{AcquiredExtensionCatalogSubmission, AcquiredExtensionPackageSubmission};

type SettlementCallback<T> = Box<dyn FnOnce(T) + Send>;

struct SharedSettlement<T> {
    done: Arc<Mutex<Option<SettlementCallback<T>>>>,
}

impl<T> Clone for SharedSettlement<T> {
    fn clone(&self) -> Self {
        Self {
            done: Arc::clone(&self.done),
        }
    }
}

impl<T: Send + 'static> SharedSettlement<T> {
    fn new(done: SettlementCallback<T>) -> Self {
        Self {
            done: Arc::new(Mutex::new(Some(done))),
        }
    }

    fn callback(&self) -> SettlementCallback<T> {
        let settlement = self.clone();
        Box::new(move |outcome| {
            settlement.settle(outcome);
        })
    }

    fn settle(&self, outcome: T) -> bool {
        let done = self
            .done
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let Some(done) = done else {
            return false;
        };
        let _ = panic::catch_unwind(AssertUnwindSafe(|| done(outcome)));
        true
    }
}

impl Shell {
    pub(super) fn observe_extension_distribution_status(
        &mut self,
        status: ExtensionDistributionStatus,
    ) {
        if self
            .extension_distribution_status
            .is_some_and(|current| status.generation() <= current.generation())
        {
            crate::diagnostic!("extensions: stale distribution status ignored");
            return;
        }
        self.extension_distribution_status = Some(status);
        self.project_extension_distribution_status();
    }

    pub(super) fn project_extension_distribution_status(&self) {
        let Some(status) = self.extension_distribution_status else {
            return;
        };
        let state = match status.state() {
            ExtensionDistributionState::Idle => ExtensionDistributionStateView::Idle,
            ExtensionDistributionState::Synchronizing => {
                ExtensionDistributionStateView::Synchronizing
            }
            ExtensionDistributionState::Ready(completion) => {
                ExtensionDistributionStateView::Ready {
                    package_count: completion.package_count(),
                    materialized_packages: completion.materialized_packages(),
                    reused_packages: completion.reused_packages(),
                    exact_retries: completion.exact_retries(),
                    newly_activated: completion.newly_activated(),
                }
            }
            ExtensionDistributionState::Failed { stage, reason } => {
                ExtensionDistributionStateView::Failed {
                    stage: distribution_stage_view(stage),
                    reason: distribution_reason_view(reason),
                }
            }
            ExtensionDistributionState::Quarantined { stage, reason } => {
                ExtensionDistributionStateView::Quarantined {
                    stage: distribution_stage_view(stage),
                    reason: distribution_reason_view(reason),
                }
            }
            ExtensionDistributionState::Shutdown => ExtensionDistributionStateView::Shutdown,
        };
        (self.emit)(Projection::ExtensionDistribution(
            ExtensionDistributionView {
                projection_revision: format!("{:032x}", self.next_projection_revision()),
                state,
            },
        ));
    }

    pub(super) fn provision_acquired_extension_package(
        &mut self,
        submission: AcquiredExtensionPackageSubmission,
    ) {
        let Some((request, deadline, done)) = submission.take() else {
            crate::diagnostic!("extensions: duplicate acquired-package submission ignored");
            return;
        };
        if std::time::Instant::now() >= deadline || !self.extension_startup_ready {
            settle_package(
                done,
                ExtensionAcquiredPackageProvisioningOutcome::Unavailable,
            );
            return;
        }
        if self.extension_lifecycle_terminal {
            settle_package(
                done,
                ExtensionAcquiredPackageProvisioningOutcome::FailedClosed,
            );
            return;
        }
        let Some(service) = self.extension_service.as_mut() else {
            settle_package(
                done,
                ExtensionAcquiredPackageProvisioningOutcome::FailedClosed,
            );
            self.fail_extension_distribution(
                ShellTerminalFailure::ExtensionDistributionLifecycleMissing,
            );
            return;
        };
        let settlement = SharedSettlement::new(done);
        let service_done: ExtensionAcquiredPackageProvisioningCallback = settlement.callback();
        let admission = panic::catch_unwind(AssertUnwindSafe(|| {
            service.begin_provision_acquired_package(request, deadline, service_done)
        }));
        match admission {
            Ok(ExtensionManagementAdmission::Accepted) => {}
            Ok(ExtensionManagementAdmission::Busy | ExtensionManagementAdmission::Unavailable) => {
                settlement.settle(ExtensionAcquiredPackageProvisioningOutcome::Unavailable);
            }
            Err(_) => {
                settlement.settle(ExtensionAcquiredPackageProvisioningOutcome::FailedClosed);
                self.fail_extension_distribution(
                    ShellTerminalFailure::ExtensionDistributionLifecyclePanicked,
                );
            }
        }
    }

    pub(super) fn activate_acquired_extension_catalog(
        &mut self,
        submission: AcquiredExtensionCatalogSubmission,
    ) {
        let Some((request, deadline, done)) = submission.take() else {
            crate::diagnostic!("extensions: duplicate acquired-catalog submission ignored");
            return;
        };
        if std::time::Instant::now() >= deadline || !self.extension_startup_ready {
            settle_catalog(done, ExtensionAcquiredCatalogActivationOutcome::Unavailable);
            return;
        }
        if self.extension_lifecycle_terminal {
            settle_catalog(
                done,
                ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
            );
            return;
        }
        let Some(service) = self.extension_service.as_mut() else {
            settle_catalog(
                done,
                ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
            );
            self.fail_extension_distribution(
                ShellTerminalFailure::ExtensionDistributionLifecycleMissing,
            );
            return;
        };
        let settlement = SharedSettlement::new(done);
        let service_done: ExtensionAcquiredCatalogActivationCallback = settlement.callback();
        let admission = panic::catch_unwind(AssertUnwindSafe(|| {
            service.begin_activate_acquired_catalog(request, deadline, service_done)
        }));
        match admission {
            Ok(ExtensionManagementAdmission::Accepted) => {}
            Ok(ExtensionManagementAdmission::Busy | ExtensionManagementAdmission::Unavailable) => {
                settlement.settle(ExtensionAcquiredCatalogActivationOutcome::Unavailable);
            }
            Err(_) => {
                settlement.settle(ExtensionAcquiredCatalogActivationOutcome::FailedClosed);
                self.fail_extension_distribution(
                    ShellTerminalFailure::ExtensionDistributionLifecyclePanicked,
                );
            }
        }
    }

    fn fail_extension_distribution(&mut self, failure: ShellTerminalFailure) {
        self.extension_lifecycle_terminal = true;
        crate::diagnostic!("extensions: acquired distribution lifecycle failed closed");
        self.report_terminal_failure(failure);
    }
}

fn distribution_stage_view(
    stage: zephium_core::ports::extensions::ExtensionDistributionFailureStage,
) -> ExtensionDistributionFailureStageView {
    use zephium_core::ports::extensions::ExtensionDistributionFailureStage as Stage;
    match stage {
        Stage::Catalog => ExtensionDistributionFailureStageView::Catalog,
        Stage::PackageFetch(index) => ExtensionDistributionFailureStageView::PackageFetch { index },
        Stage::PackageProvision(index) => {
            ExtensionDistributionFailureStageView::PackageProvision { index }
        }
        Stage::CatalogActivation => ExtensionDistributionFailureStageView::CatalogActivation,
    }
}

fn distribution_reason_view(
    reason: zephium_core::ports::extensions::ExtensionDistributionFailureReason,
) -> ExtensionDistributionFailureReasonView {
    use zephium_core::ports::extensions::ExtensionDistributionFailureReason as Reason;
    match reason {
        Reason::Acquisition => ExtensionDistributionFailureReasonView::Acquisition,
        Reason::Busy => ExtensionDistributionFailureReasonView::Busy,
        Reason::ServiceUnavailable => ExtensionDistributionFailureReasonView::ServiceUnavailable,
        Reason::ServiceRejected => ExtensionDistributionFailureReasonView::ServiceRejected,
        Reason::ServiceFailedClosed => ExtensionDistributionFailureReasonView::ServiceFailedClosed,
        Reason::SettlementTimedOut => ExtensionDistributionFailureReasonView::SettlementTimedOut,
        Reason::SettlementLost => ExtensionDistributionFailureReasonView::SettlementLost,
        Reason::SubmissionPanicked => ExtensionDistributionFailureReasonView::SubmissionPanicked,
        Reason::OutcomeUnresolved => ExtensionDistributionFailureReasonView::OutcomeUnresolved,
        Reason::ActivationRejected => ExtensionDistributionFailureReasonView::ActivationRejected,
        Reason::Accounting => ExtensionDistributionFailureReasonView::Accounting,
    }
}

fn settle_package(
    done: ExtensionAcquiredPackageProvisioningCallback,
    outcome: ExtensionAcquiredPackageProvisioningOutcome,
) {
    SharedSettlement::new(done).settle(outcome);
}

fn settle_catalog(
    done: ExtensionAcquiredCatalogActivationCallback,
    outcome: ExtensionAcquiredCatalogActivationOutcome,
) {
    SharedSettlement::new(done).settle(outcome);
}
