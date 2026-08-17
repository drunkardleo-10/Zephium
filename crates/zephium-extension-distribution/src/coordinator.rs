use std::fmt;
use std::mem::size_of;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::oneshot;
use zephium_core::extensions::ExtensionCatalogSetDigest;
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationCallback, ExtensionAcquiredCatalogActivationOutcome,
    ExtensionAcquiredCatalogActivationRequest, ExtensionAcquiredPackageProvisioningCallback,
    ExtensionAcquiredPackageProvisioningOutcome, ExtensionAcquiredPackageProvisioningRequest,
    ExtensionAcquiredRuntimeSelection, ExtensionManagementAdmission,
    MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES,
};
use zephium_extension_package::{ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision};

use crate::authentication::CatalogAuthenticator;
use crate::client::{ArtifactTransport, DistributionClient};
use crate::{
    ExtensionDistributionClient, ExtensionDistributionError,
    MAX_EXTENSION_DISTRIBUTION_SESSION_RETAINED_BYTES,
};

const COORDINATOR_IDLE: u8 = 0;
const COORDINATOR_ACTIVE: u8 = 1;
const COORDINATOR_QUARANTINED: u8 = 2;
const SERVICE_OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
const SERVICE_OBSERVATION_GRACE: Duration = Duration::from_secs(5);
const MAX_OUTCOME_UNKNOWN_RETRIES: u8 = 1;

/// Maximum logical bytes retained while one distribution run is active.
///
/// This includes the authenticated session and the one move-only package or
/// activation request being transferred to the service. The coordinator never
/// fetches a later package until the prior callback has settled and the service
/// has released that request's admission charge.
pub const MAX_EXTENSION_DISTRIBUTION_COORDINATOR_RETAINED_BYTES: usize =
    MAX_EXTENSION_DISTRIBUTION_SESSION_RETAINED_BYTES
        + MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES
        + 4 * 1024;

const _: () = assert!(
    MAX_EXTENSION_DISTRIBUTION_COORDINATOR_RETAINED_BYTES
        >= MAX_EXTENSION_DISTRIBUTION_SESSION_RETAINED_BYTES
            + MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES
);

/// Non-blocking Shell/service submission boundary used by distribution.
///
/// Implementations enqueue onto the Shell-owned actor; they must not borrow
/// the extension-service lifecycle across network awaits. `Accepted` transfers
/// callback ownership and requires exactly one eventual invocation. `Busy` or
/// `Unavailable` must drop the callback without invoking it and leave durable
/// state unchanged.
pub trait ExtensionDistributionServicePort: Send + Sync {
    /// Enqueues one already authenticated, move-owned package request.
    fn begin_provision_acquired_package(
        &self,
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: Instant,
        done: ExtensionAcquiredPackageProvisioningCallback,
    ) -> ExtensionManagementAdmission;

    /// Enqueues source-free activation of the complete materialized catalog.
    fn begin_activate_acquired_catalog(
        &self,
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: Instant,
        done: ExtensionAcquiredCatalogActivationCallback,
    ) -> ExtensionManagementAdmission;
}

/// Stable phase for one redacted distribution failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionDistributionFailurePhase {
    /// Catalog acquisition or coordinator admission.
    Catalog,
    /// Fetching and authenticating one stable catalog package index.
    PackageFetch(u8),
    /// Durable provisioning of one stable catalog package index.
    PackageProvision(u8),
    /// Source-free activation of the complete catalog selection.
    CatalogActivation,
}

impl fmt::Display for ExtensionDistributionFailurePhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Catalog => formatter.write_str("catalog acquisition"),
            Self::PackageFetch(index) => write!(formatter, "package {index} acquisition"),
            Self::PackageProvision(index) => write!(formatter, "package {index} provisioning"),
            Self::CatalogActivation => formatter.write_str("catalog activation"),
        }
    }
}

/// Stable, source-free reason for one distribution failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum ExtensionDistributionFailureReason {
    /// Catalog, CRX, legal-object, or fixed-origin acquisition failed.
    #[error("authenticated acquisition failed: {0}")]
    Acquisition(#[from] ExtensionDistributionError),
    /// Another run already owns the one global distribution slot.
    #[error("another extension distribution run is active")]
    CoordinatorBusy,
    /// A prior accepted operation lost settlement proof; restart is required.
    #[error("extension distribution is quarantined until restart")]
    CoordinatorQuarantined,
    /// Shell or the extension service could not admit the operation.
    #[error("the extension service is unavailable")]
    ServiceUnavailable,
    /// The service independently rejected authenticated input or projection.
    #[error("the extension service rejected the distribution input")]
    ServiceRejected,
    /// The service or submission port violated a security invariant.
    #[error("the extension service failed closed")]
    ServiceFailedClosed,
    /// An accepted callback was not observed before its hard bound.
    #[error("extension service settlement timed out")]
    SettlementTimedOut,
    /// An accepted callback channel closed without a terminal outcome.
    #[error("extension service settlement was lost")]
    SettlementLost,
    /// A non-blocking submission port panicked at its trust boundary.
    #[error("extension service submission panicked")]
    SubmissionPanicked,
    /// One exact retry could not resolve a durable outcome.
    #[error("extension distribution outcome remained unknown after exact retry")]
    OutcomeUnresolved,
    /// A source-free activation request violated its bounded transport shape.
    #[error("catalog activation request was rejected")]
    ActivationRequestRejected,
    /// Internal bounded accounting could not advance without overflow.
    #[error("extension distribution accounting failed closed")]
    Accounting,
}

impl ExtensionDistributionFailureReason {
    const fn requires_quarantine(self) -> bool {
        matches!(
            self,
            Self::CoordinatorQuarantined
                | Self::ServiceFailedClosed
                | Self::SettlementTimedOut
                | Self::SettlementLost
                | Self::SubmissionPanicked
                | Self::OutcomeUnresolved
                | Self::Accounting
        )
    }
}

/// Redacted failure from one bounded distribution run.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("extension distribution failed during {phase}: {reason}")]
pub struct ExtensionDistributionFailure {
    phase: ExtensionDistributionFailurePhase,
    reason: ExtensionDistributionFailureReason,
}

impl ExtensionDistributionFailure {
    const fn new(
        phase: ExtensionDistributionFailurePhase,
        reason: ExtensionDistributionFailureReason,
    ) -> Self {
        Self { phase, reason }
    }

    /// Returns the exact bounded orchestration phase.
    pub const fn phase(self) -> ExtensionDistributionFailurePhase {
        self.phase
    }

    /// Returns the stable source-free failure reason.
    pub const fn reason(self) -> ExtensionDistributionFailureReason {
        self.reason
    }
}

/// Successful settlement of one complete product-selected catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionDistributionCompletion {
    catalog_revision: ExtensionReleaseCatalogRevision,
    catalog_digest: ExtensionReleaseCatalogDigest,
    catalog_set: ExtensionCatalogSetDigest,
    package_count: u8,
    materialized_packages: u8,
    reused_packages: u8,
    exact_retries: u8,
    newly_activated: bool,
}

impl ExtensionDistributionCompletion {
    /// Returns the authenticated release-catalog revision.
    pub const fn catalog_revision(self) -> ExtensionReleaseCatalogRevision {
        self.catalog_revision
    }

    /// Returns the authenticated exact-catalog digest.
    pub const fn catalog_digest(self) -> ExtensionReleaseCatalogDigest {
        self.catalog_digest
    }

    /// Returns the durable complete catalog-set identity.
    pub const fn catalog_set(self) -> ExtensionCatalogSetDigest {
        self.catalog_set
    }

    /// Returns the number of selected package rows.
    pub const fn package_count(self) -> u8 {
        self.package_count
    }

    /// Returns the number of packages newly materialized in this run.
    pub const fn materialized_packages(self) -> u8 {
        self.materialized_packages
    }

    /// Returns the number of fully reverified package replays.
    pub const fn reused_packages(self) -> u8 {
        self.reused_packages
    }

    /// Returns the number of exact outcome-unknown recovery retries.
    pub const fn exact_retries(self) -> u8 {
        self.exact_retries
    }

    /// Returns whether this run promoted rather than reverified current state.
    pub const fn newly_activated(self) -> bool {
        self.newly_activated
    }
}

/// Single-flight product distribution coordinator.
///
/// The coordinator performs network work on its caller's async executor and
/// crosses into Shell only through [`ExtensionDistributionServicePort`]. It
/// never runs on or retains the native event-loop owner, never holds two
/// package requests, and stops before fetching more data whenever service
/// settlement is unavailable or unproven.
pub struct ExtensionDistributionCoordinator {
    client: ExtensionDistributionClient,
    service: Arc<dyn ExtensionDistributionServicePort>,
    state: AtomicU8,
    timing: CoordinatorTiming,
}

impl ExtensionDistributionCoordinator {
    /// Binds one authenticated fixed-origin client to a non-blocking service port.
    pub fn new(
        client: ExtensionDistributionClient,
        service: Arc<dyn ExtensionDistributionServicePort>,
    ) -> Self {
        Self {
            client,
            service,
            state: AtomicU8::new(COORDINATOR_IDLE),
            timing: CoordinatorTiming::production(),
        }
    }

    /// Fetches, provisions, and activates one complete selected catalog.
    ///
    /// Concurrent calls fail before network I/O. A lost or timed-out accepted
    /// settlement quarantines this coordinator for the process lifetime,
    /// because another request could otherwise overlap still-owned bytes.
    pub async fn synchronize(
        &self,
        selections: Vec<ExtensionAcquiredRuntimeSelection>,
    ) -> Result<ExtensionDistributionCompletion, ExtensionDistributionFailure> {
        let mut admission = RunAdmission::try_acquire(&self.state)?;
        let result = synchronize_client(
            &self.client.inner,
            self.service.as_ref(),
            selections,
            self.timing,
            &mut admission,
        )
        .await;
        if matches!(result, Err(failure) if failure.reason().requires_quarantine()) {
            admission.quarantine();
        }
        result
    }

    /// Returns whether settlement loss permanently stopped this instance.
    pub fn is_quarantined(&self) -> bool {
        self.state.load(Ordering::Acquire) == COORDINATOR_QUARANTINED
    }
}

impl fmt::Debug for ExtensionDistributionCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionDistributionCoordinator")
            .field("client", &self.client)
            .field("quarantined", &self.is_quarantined())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
struct CoordinatorTiming {
    operation: Duration,
    observation: Duration,
}

impl CoordinatorTiming {
    const fn production() -> Self {
        Self {
            operation: SERVICE_OPERATION_TIMEOUT,
            observation: Duration::from_secs(
                SERVICE_OPERATION_TIMEOUT.as_secs() + SERVICE_OBSERVATION_GRACE.as_secs(),
            ),
        }
    }
}

struct RunAdmission<'a> {
    state: &'a AtomicU8,
    quarantined: bool,
    settlement_pending: bool,
}

impl<'a> RunAdmission<'a> {
    fn try_acquire(state: &'a AtomicU8) -> Result<Self, ExtensionDistributionFailure> {
        match state.compare_exchange(
            COORDINATOR_IDLE,
            COORDINATOR_ACTIVE,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(Self {
                state,
                quarantined: false,
                settlement_pending: false,
            }),
            Err(COORDINATOR_ACTIVE) => Err(ExtensionDistributionFailure::new(
                ExtensionDistributionFailurePhase::Catalog,
                ExtensionDistributionFailureReason::CoordinatorBusy,
            )),
            Err(_) => Err(ExtensionDistributionFailure::new(
                ExtensionDistributionFailurePhase::Catalog,
                ExtensionDistributionFailureReason::CoordinatorQuarantined,
            )),
        }
    }

    fn quarantine(&mut self) {
        self.quarantined = true;
        self.state.store(COORDINATOR_QUARANTINED, Ordering::Release);
    }

    fn begin_settlement(&mut self) {
        debug_assert!(!self.settlement_pending);
        self.settlement_pending = true;
    }

    fn complete_settlement(&mut self) {
        debug_assert!(self.settlement_pending);
        self.settlement_pending = false;
    }
}

impl Drop for RunAdmission<'_> {
    fn drop(&mut self) {
        if self.quarantined || self.settlement_pending {
            self.state.store(COORDINATOR_QUARANTINED, Ordering::Release);
        } else {
            self.state.store(COORDINATOR_IDLE, Ordering::Release);
        }
    }
}

async fn synchronize_client<T, A>(
    client: &DistributionClient<T, A>,
    service: &dyn ExtensionDistributionServicePort,
    selections: Vec<ExtensionAcquiredRuntimeSelection>,
    timing: CoordinatorTiming,
    admission: &mut RunAdmission<'_>,
) -> Result<ExtensionDistributionCompletion, ExtensionDistributionFailure>
where
    T: ArtifactTransport + Sync,
    A: CatalogAuthenticator,
{
    let session = client.begin(selections).await.map_err(|error| {
        ExtensionDistributionFailure::new(
            ExtensionDistributionFailurePhase::Catalog,
            ExtensionDistributionFailureReason::Acquisition(error),
        )
    })?;
    let package_count = u8::try_from(session.package_count()).map_err(|_| {
        ExtensionDistributionFailure::new(
            ExtensionDistributionFailurePhase::Catalog,
            ExtensionDistributionFailureReason::Accounting,
        )
    })?;
    let mut materialized_packages = 0_u8;
    let mut reused_packages = 0_u8;
    let mut exact_retries = 0_u8;

    for index in 0..session.package_count() {
        let phase_index = u8::try_from(index).map_err(|_| {
            ExtensionDistributionFailure::new(
                ExtensionDistributionFailurePhase::Catalog,
                ExtensionDistributionFailureReason::Accounting,
            )
        })?;
        let mut retries = 0_u8;
        loop {
            let request = client
                .fetch_package(&session, index)
                .await
                .map_err(|error| {
                    ExtensionDistributionFailure::new(
                        ExtensionDistributionFailurePhase::PackageFetch(phase_index),
                        ExtensionDistributionFailureReason::Acquisition(error),
                    )
                })?;
            let outcome = observe_package(service, request, timing, admission)
                .await
                .map_err(|reason| {
                    ExtensionDistributionFailure::new(
                        ExtensionDistributionFailurePhase::PackageProvision(phase_index),
                        reason,
                    )
                })?;
            match outcome {
                ExtensionAcquiredPackageProvisioningOutcome::Materialized => {
                    materialized_packages = increment(materialized_packages)?;
                    break;
                }
                ExtensionAcquiredPackageProvisioningOutcome::AlreadyMaterialized => {
                    reused_packages = increment(reused_packages)?;
                    break;
                }
                ExtensionAcquiredPackageProvisioningOutcome::Rejected => {
                    return Err(ExtensionDistributionFailure::new(
                        ExtensionDistributionFailurePhase::PackageProvision(phase_index),
                        ExtensionDistributionFailureReason::ServiceRejected,
                    ));
                }
                ExtensionAcquiredPackageProvisioningOutcome::Unavailable => {
                    return Err(ExtensionDistributionFailure::new(
                        ExtensionDistributionFailurePhase::PackageProvision(phase_index),
                        ExtensionDistributionFailureReason::ServiceUnavailable,
                    ));
                }
                ExtensionAcquiredPackageProvisioningOutcome::FailedClosed => {
                    return Err(ExtensionDistributionFailure::new(
                        ExtensionDistributionFailurePhase::PackageProvision(phase_index),
                        ExtensionDistributionFailureReason::ServiceFailedClosed,
                    ));
                }
                ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown
                    if retries < MAX_OUTCOME_UNKNOWN_RETRIES =>
                {
                    retries = increment(retries)?;
                    exact_retries = increment(exact_retries)?;
                }
                ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown => {
                    return Err(ExtensionDistributionFailure::new(
                        ExtensionDistributionFailurePhase::PackageProvision(phase_index),
                        ExtensionDistributionFailureReason::OutcomeUnresolved,
                    ));
                }
            }
        }
    }

    let mut retries = 0_u8;
    loop {
        let request = session.activation_request().map_err(|_| {
            ExtensionDistributionFailure::new(
                ExtensionDistributionFailurePhase::CatalogActivation,
                ExtensionDistributionFailureReason::ActivationRequestRejected,
            )
        })?;
        let outcome = observe_activation(service, request, timing, admission)
            .await
            .map_err(|reason| {
                ExtensionDistributionFailure::new(
                    ExtensionDistributionFailurePhase::CatalogActivation,
                    reason,
                )
            })?;
        let (catalog_set, newly_activated) = match outcome {
            ExtensionAcquiredCatalogActivationOutcome::Activated(catalog_set) => {
                (catalog_set, true)
            }
            ExtensionAcquiredCatalogActivationOutcome::AlreadyActive(catalog_set) => {
                (catalog_set, false)
            }
            ExtensionAcquiredCatalogActivationOutcome::Rejected => {
                return Err(ExtensionDistributionFailure::new(
                    ExtensionDistributionFailurePhase::CatalogActivation,
                    ExtensionDistributionFailureReason::ServiceRejected,
                ));
            }
            ExtensionAcquiredCatalogActivationOutcome::Unavailable => {
                return Err(ExtensionDistributionFailure::new(
                    ExtensionDistributionFailurePhase::CatalogActivation,
                    ExtensionDistributionFailureReason::ServiceUnavailable,
                ));
            }
            ExtensionAcquiredCatalogActivationOutcome::FailedClosed => {
                return Err(ExtensionDistributionFailure::new(
                    ExtensionDistributionFailurePhase::CatalogActivation,
                    ExtensionDistributionFailureReason::ServiceFailedClosed,
                ));
            }
            ExtensionAcquiredCatalogActivationOutcome::OutcomeUnknown
                if retries < MAX_OUTCOME_UNKNOWN_RETRIES =>
            {
                retries = increment(retries)?;
                exact_retries = increment(exact_retries)?;
                continue;
            }
            ExtensionAcquiredCatalogActivationOutcome::OutcomeUnknown => {
                return Err(ExtensionDistributionFailure::new(
                    ExtensionDistributionFailurePhase::CatalogActivation,
                    ExtensionDistributionFailureReason::OutcomeUnresolved,
                ));
            }
        };
        return Ok(ExtensionDistributionCompletion {
            catalog_revision: session.catalog_revision(),
            catalog_digest: session.catalog_digest(),
            catalog_set,
            package_count,
            materialized_packages,
            reused_packages,
            exact_retries,
            newly_activated,
        });
    }
}

fn increment(value: u8) -> Result<u8, ExtensionDistributionFailure> {
    value.checked_add(1).ok_or_else(|| {
        ExtensionDistributionFailure::new(
            ExtensionDistributionFailurePhase::Catalog,
            ExtensionDistributionFailureReason::Accounting,
        )
    })
}

async fn observe_package(
    service: &dyn ExtensionDistributionServicePort,
    request: ExtensionAcquiredPackageProvisioningRequest,
    timing: CoordinatorTiming,
    run: &mut RunAdmission<'_>,
) -> Result<ExtensionAcquiredPackageProvisioningOutcome, ExtensionDistributionFailureReason> {
    let deadline = Instant::now()
        .checked_add(timing.operation)
        .ok_or(ExtensionDistributionFailureReason::Accounting)?;
    let (sender, receiver) = oneshot::channel();
    let done: ExtensionAcquiredPackageProvisioningCallback = Box::new(move |outcome| {
        let _ = sender.send(outcome);
    });
    let admission = panic::catch_unwind(AssertUnwindSafe(|| {
        service.begin_provision_acquired_package(request, deadline, done)
    }))
    .map_err(|_| ExtensionDistributionFailureReason::SubmissionPanicked)?;
    observe_admitted(admission, receiver, timing.observation, run).await
}

async fn observe_activation(
    service: &dyn ExtensionDistributionServicePort,
    request: ExtensionAcquiredCatalogActivationRequest,
    timing: CoordinatorTiming,
    run: &mut RunAdmission<'_>,
) -> Result<ExtensionAcquiredCatalogActivationOutcome, ExtensionDistributionFailureReason> {
    let deadline = Instant::now()
        .checked_add(timing.operation)
        .ok_or(ExtensionDistributionFailureReason::Accounting)?;
    let (sender, receiver) = oneshot::channel();
    let done: ExtensionAcquiredCatalogActivationCallback = Box::new(move |outcome| {
        let _ = sender.send(outcome);
    });
    let admission = panic::catch_unwind(AssertUnwindSafe(|| {
        service.begin_activate_acquired_catalog(request, deadline, done)
    }))
    .map_err(|_| ExtensionDistributionFailureReason::SubmissionPanicked)?;
    observe_admitted(admission, receiver, timing.observation, run).await
}

async fn observe_admitted<T>(
    admission: ExtensionManagementAdmission,
    receiver: oneshot::Receiver<T>,
    observation: Duration,
    run: &mut RunAdmission<'_>,
) -> Result<T, ExtensionDistributionFailureReason> {
    match admission {
        ExtensionManagementAdmission::Accepted => {
            run.begin_settlement();
            match tokio::time::timeout(observation, receiver).await {
                Ok(Ok(outcome)) => {
                    run.complete_settlement();
                    Ok(outcome)
                }
                Ok(Err(_)) => Err(ExtensionDistributionFailureReason::SettlementLost),
                Err(_) => Err(ExtensionDistributionFailureReason::SettlementTimedOut),
            }
        }
        ExtensionManagementAdmission::Busy | ExtensionManagementAdmission::Unavailable => {
            drop(receiver);
            Err(ExtensionDistributionFailureReason::ServiceUnavailable)
        }
    }
}

const _: () = assert!(size_of::<ExtensionDistributionCompletion>() <= 128);

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    use zephium_core::ports::extensions::ExtensionAcquiredRuntimeProfile;
    use zephium_extension_package::MAX_CRX3_HEADER_BYTES;

    use super::*;
    use crate::authentication::StructuralTestCatalogAuthenticator;
    use crate::client::tests::{fixture, ExpectedFetch, FakeTransport, Fixture};

    struct FakeService {
        package_outcomes: Mutex<VecDeque<ExtensionAcquiredPackageProvisioningOutcome>>,
        activation_outcomes: Mutex<VecDeque<ExtensionAcquiredCatalogActivationOutcome>>,
        package_calls: AtomicUsize,
        activation_calls: AtomicUsize,
    }

    impl FakeService {
        fn new(
            package_outcomes: impl IntoIterator<Item = ExtensionAcquiredPackageProvisioningOutcome>,
            activation_outcomes: impl IntoIterator<Item = ExtensionAcquiredCatalogActivationOutcome>,
        ) -> Self {
            Self {
                package_outcomes: Mutex::new(package_outcomes.into_iter().collect()),
                activation_outcomes: Mutex::new(activation_outcomes.into_iter().collect()),
                package_calls: AtomicUsize::new(0),
                activation_calls: AtomicUsize::new(0),
            }
        }
    }

    impl ExtensionDistributionServicePort for FakeService {
        fn begin_provision_acquired_package(
            &self,
            _request: ExtensionAcquiredPackageProvisioningRequest,
            deadline: Instant,
            done: ExtensionAcquiredPackageProvisioningCallback,
        ) -> ExtensionManagementAdmission {
            assert!(deadline > Instant::now());
            self.package_calls.fetch_add(1, AtomicOrdering::AcqRel);
            let outcome = self.package_outcomes.lock().unwrap().pop_front().unwrap();
            done(outcome);
            ExtensionManagementAdmission::Accepted
        }

        fn begin_activate_acquired_catalog(
            &self,
            _request: ExtensionAcquiredCatalogActivationRequest,
            deadline: Instant,
            done: ExtensionAcquiredCatalogActivationCallback,
        ) -> ExtensionManagementAdmission {
            assert!(deadline > Instant::now());
            assert!(self.package_outcomes.lock().unwrap().is_empty());
            self.activation_calls.fetch_add(1, AtomicOrdering::AcqRel);
            let outcome = self
                .activation_outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap();
            done(outcome);
            ExtensionManagementAdmission::Accepted
        }
    }

    struct LostCallbackService;

    impl ExtensionDistributionServicePort for LostCallbackService {
        fn begin_provision_acquired_package(
            &self,
            _request: ExtensionAcquiredPackageProvisioningRequest,
            _deadline: Instant,
            _done: ExtensionAcquiredPackageProvisioningCallback,
        ) -> ExtensionManagementAdmission {
            ExtensionManagementAdmission::Accepted
        }

        fn begin_activate_acquired_catalog(
            &self,
            _request: ExtensionAcquiredCatalogActivationRequest,
            _deadline: Instant,
            _done: ExtensionAcquiredCatalogActivationCallback,
        ) -> ExtensionManagementAdmission {
            ExtensionManagementAdmission::Accepted
        }
    }

    struct HeldCallbackService {
        callback: Mutex<Option<ExtensionAcquiredPackageProvisioningCallback>>,
    }

    impl ExtensionDistributionServicePort for HeldCallbackService {
        fn begin_provision_acquired_package(
            &self,
            _request: ExtensionAcquiredPackageProvisioningRequest,
            _deadline: Instant,
            done: ExtensionAcquiredPackageProvisioningCallback,
        ) -> ExtensionManagementAdmission {
            *self.callback.lock().unwrap() = Some(done);
            ExtensionManagementAdmission::Accepted
        }

        fn begin_activate_acquired_catalog(
            &self,
            _request: ExtensionAcquiredCatalogActivationRequest,
            _deadline: Instant,
            _done: ExtensionAcquiredCatalogActivationCallback,
        ) -> ExtensionManagementAdmission {
            unreachable!("the held package callback prevents activation")
        }
    }

    struct PanickingService;

    impl ExtensionDistributionServicePort for PanickingService {
        fn begin_provision_acquired_package(
            &self,
            _request: ExtensionAcquiredPackageProvisioningRequest,
            _deadline: Instant,
            _done: ExtensionAcquiredPackageProvisioningCallback,
        ) -> ExtensionManagementAdmission {
            panic!("submission boundary fault")
        }

        fn begin_activate_acquired_catalog(
            &self,
            _request: ExtensionAcquiredCatalogActivationRequest,
            _deadline: Instant,
            _done: ExtensionAcquiredCatalogActivationCallback,
        ) -> ExtensionManagementAdmission {
            unreachable!("the package submission panics first")
        }
    }

    #[test]
    fn single_flight_admission_is_fail_fast_and_quarantine_is_terminal() {
        let state = AtomicU8::new(COORDINATOR_IDLE);
        let mut first = RunAdmission::try_acquire(&state).unwrap();
        let busy = match RunAdmission::try_acquire(&state) {
            Ok(_) => panic!("a second run must not be admitted"),
            Err(error) => error,
        };
        assert_eq!(
            busy.reason(),
            ExtensionDistributionFailureReason::CoordinatorBusy
        );
        first.quarantine();
        drop(first);
        let quarantined = match RunAdmission::try_acquire(&state) {
            Ok(_) => panic!("a quarantined coordinator must remain closed"),
            Err(error) => error,
        };
        assert_eq!(
            quarantined.reason(),
            ExtensionDistributionFailureReason::CoordinatorQuarantined
        );
    }

    #[test]
    fn public_coordinator_values_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_send<T: Send>() {}

        assert_send_sync::<ExtensionDistributionCoordinator>();
        assert_send_sync::<ExtensionDistributionCompletion>();
        assert_send_sync::<ExtensionDistributionFailure>();
        assert_send::<Box<dyn ExtensionDistributionServicePort>>();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refused_admission_does_not_wait_for_a_callback() {
        let (sender, receiver) = oneshot::channel::<()>();
        drop(sender);
        let state = AtomicU8::new(COORDINATOR_IDLE);
        let mut run = RunAdmission::try_acquire(&state).unwrap();
        assert_eq!(
            observe_admitted(
                ExtensionManagementAdmission::Busy,
                receiver,
                Duration::from_secs(60),
                &mut run,
            )
            .await
            .unwrap_err(),
            ExtensionDistributionFailureReason::ServiceUnavailable
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn complete_run_materializes_then_activates_exactly_once() {
        let (client, selection) = client_with_package_attempts(1);
        let catalog_set = ExtensionCatalogSetDigest::from_bytes([9; 32]);
        let service = FakeService::new(
            [ExtensionAcquiredPackageProvisioningOutcome::Materialized],
            [ExtensionAcquiredCatalogActivationOutcome::Activated(
                catalog_set,
            )],
        );

        let completion = synchronize_test(
            &client,
            &service,
            vec![selection],
            CoordinatorTiming::production(),
        )
        .await
        .unwrap();

        assert_eq!(completion.catalog_set(), catalog_set);
        assert_eq!(completion.package_count(), 1);
        assert_eq!(completion.materialized_packages(), 1);
        assert_eq!(completion.reused_packages(), 0);
        assert_eq!(completion.exact_retries(), 0);
        assert!(completion.newly_activated());
        assert_eq!(service.package_calls.load(AtomicOrdering::Acquire), 1);
        assert_eq!(service.activation_calls.load(AtomicOrdering::Acquire), 1);
        assert!(client.transport.expected.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn completed_unknowns_retry_exact_package_and_source_free_activation_once() {
        let (client, selection) = client_with_package_attempts(2);
        let catalog_set = ExtensionCatalogSetDigest::from_bytes([10; 32]);
        let service = FakeService::new(
            [
                ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown,
                ExtensionAcquiredPackageProvisioningOutcome::AlreadyMaterialized,
            ],
            [
                ExtensionAcquiredCatalogActivationOutcome::OutcomeUnknown,
                ExtensionAcquiredCatalogActivationOutcome::AlreadyActive(catalog_set),
            ],
        );

        let completion = synchronize_test(
            &client,
            &service,
            vec![selection],
            CoordinatorTiming::production(),
        )
        .await
        .unwrap();

        assert_eq!(completion.materialized_packages(), 0);
        assert_eq!(completion.reused_packages(), 1);
        assert_eq!(completion.exact_retries(), 2);
        assert!(!completion.newly_activated());
        assert_eq!(service.package_calls.load(AtomicOrdering::Acquire), 2);
        assert_eq!(service.activation_calls.load(AtomicOrdering::Acquire), 2);
        assert!(client.transport.expected.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_second_unknown_stops_before_activation() {
        let (client, selection) = client_with_package_attempts(2);
        let service = FakeService::new(
            [
                ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown,
                ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown,
            ],
            [],
        );

        let failure = synchronize_test(
            &client,
            &service,
            vec![selection],
            CoordinatorTiming::production(),
        )
        .await
        .unwrap_err();

        assert_eq!(
            failure.phase(),
            ExtensionDistributionFailurePhase::PackageProvision(0)
        );
        assert_eq!(
            failure.reason(),
            ExtensionDistributionFailureReason::OutcomeUnresolved
        );
        assert_eq!(service.activation_calls.load(AtomicOrdering::Acquire), 0);
        assert!(client.transport.expected.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn callback_loss_fails_before_any_later_fetch() {
        let (client, selection) = client_with_package_attempts(1);
        let failure = synchronize_test(
            &client,
            &LostCallbackService,
            vec![selection],
            CoordinatorTiming::production(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            failure.reason(),
            ExtensionDistributionFailureReason::SettlementLost
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn callback_timeout_is_bounded_and_submission_panics_are_contained() {
        let (client, selection) = client_with_package_attempts(1);
        let held = HeldCallbackService {
            callback: Mutex::new(None),
        };
        let failure = synchronize_test(
            &client,
            &held,
            vec![selection],
            CoordinatorTiming {
                operation: Duration::from_millis(1),
                observation: Duration::from_millis(2),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            failure.reason(),
            ExtensionDistributionFailureReason::SettlementTimedOut
        );
        drop(held.callback.lock().unwrap().take());

        let (client, selection) = client_with_package_attempts(1);
        let failure = synchronize_test(
            &client,
            &PanickingService,
            vec![selection],
            CoordinatorTiming::production(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            failure.reason(),
            ExtensionDistributionFailureReason::SubmissionPanicked
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelling_an_accepted_unsettled_callback_quarantines_the_run() {
        let state = AtomicU8::new(COORDINATOR_IDLE);
        let mut admission = RunAdmission::try_acquire(&state).unwrap();
        let service = HeldCallbackService {
            callback: Mutex::new(None),
        };
        let request = ExtensionAcquiredPackageProvisioningRequest::new_for_profile(
            vec![1],
            zephium_core::extensions::ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionAcquiredRuntimeProfile::MacosNative,
            vec![3],
            vec![4],
        )
        .unwrap();
        let mut observation = Box::pin(observe_package(
            &service,
            request,
            CoordinatorTiming {
                operation: Duration::from_secs(60),
                observation: Duration::from_secs(60),
            },
            &mut admission,
        ));

        assert!(
            tokio::time::timeout(Duration::from_millis(2), observation.as_mut())
                .await
                .is_err()
        );
        drop(observation);
        drop(admission);
        assert_eq!(state.load(Ordering::Acquire), COORDINATOR_QUARANTINED);
        drop(service.callback.lock().unwrap().take());
    }

    async fn synchronize_test<T, A>(
        client: &DistributionClient<T, A>,
        service: &dyn ExtensionDistributionServicePort,
        selections: Vec<ExtensionAcquiredRuntimeSelection>,
        timing: CoordinatorTiming,
    ) -> Result<ExtensionDistributionCompletion, ExtensionDistributionFailure>
    where
        T: ArtifactTransport + Sync,
        A: CatalogAuthenticator,
    {
        let state = AtomicU8::new(COORDINATOR_IDLE);
        let mut admission = RunAdmission::try_acquire(&state).unwrap();
        let result = synchronize_client(client, service, selections, timing, &mut admission).await;
        if matches!(result, Err(failure) if failure.reason().requires_quarantine()) {
            admission.quarantine();
        }
        result
    }

    fn client_with_package_attempts(
        package_attempts: usize,
    ) -> (
        DistributionClient<FakeTransport, StructuralTestCatalogAuthenticator>,
        ExtensionAcquiredRuntimeSelection,
    ) {
        let fixture = fixture();
        let metadata = url::Url::parse("https://metadata.example/extensions/stable/").unwrap();
        let targets = url::Url::parse("https://objects.example/extensions/stable/").unwrap();
        let catalog_url = crate::layout::catalog_url(&metadata).unwrap();
        let mut expected = VecDeque::from([ExpectedFetch {
            url: catalog_url.clone(),
            max_bytes: zephium_core::ports::extensions::MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
            bytes: fixture.catalog.clone(),
        }]);
        append_package_attempts(&mut expected, &targets, &fixture, package_attempts);
        let client = DistributionClient {
            transport: FakeTransport {
                expected: Mutex::new(expected),
            },
            authenticator: StructuralTestCatalogAuthenticator,
            catalog_url,
            targets_base: targets,
        };
        let selection = ExtensionAcquiredRuntimeSelection::new_for_profile(
            fixture.package_key,
            ExtensionAcquiredRuntimeProfile::MacosNative,
        );
        (client, selection)
    }

    fn append_package_attempts(
        expected: &mut VecDeque<ExpectedFetch>,
        targets: &url::Url,
        fixture: &Fixture,
        attempts: usize,
    ) {
        let legal_url = crate::layout::legal_notice_url(targets, &fixture.legal_sha256).unwrap();
        let crx_url = crate::layout::crx3_url(
            targets,
            fixture.package_key,
            zephium_core::extensions::ExtensionPackageRevision::INITIAL,
            zephium_core::extensions::ExtensionArchiveDigest::from_bytes(fixture.archive_sha256),
        )
        .unwrap();
        for _ in 0..attempts {
            expected.push_back(ExpectedFetch {
                url: legal_url.clone(),
                max_bytes: fixture.legal.len(),
                bytes: fixture.legal.clone(),
            });
            expected.push_back(ExpectedFetch {
                url: crx_url.clone(),
                max_bytes: fixture.archive_length + MAX_CRX3_HEADER_BYTES + 12,
                bytes: fixture.crx.clone(),
            });
        }
    }
}
