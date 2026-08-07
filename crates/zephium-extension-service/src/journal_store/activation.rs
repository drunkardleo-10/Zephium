//! Fenced fresh-activation transactions over the complete journal projection.

use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionExpectedNativeOwnershipIdentity, ExtensionManifestDescriptor,
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipEntryCas,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipJournal,
    ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipKey,
    ExtensionNativeOwnershipPhase, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeEligibilityDenial, MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_RETAINED_BYTES,
};
use zephium_core::ports::store::{
    ExtensionNativeOwnershipActivationOutcome, ExtensionNativeOwnershipActivationStale,
};
use zephium_extension_repository::BundledRuntimePackageAccessBuildError;
use zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES;
use zephium_store::{ExtensionServiceStoreAuthority, ExtensionServiceStoreCallOutcome};

use super::{applied_matches, JournalBackend, JournalProjection};
use crate::repository::{ServiceRuntimeAcquisitionPlan, ServiceRuntimePackageAccess};

const RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();
const TEMPORARY_MANIFEST_HANDLE_BYTES: usize = size_of::<Arc<ExtensionManifestDescriptor>>();

const fn ambiguity_additional_retained_bytes<Authority>() -> usize {
    size_of::<JournalActivationReloadRequired<Authority>>()
        .saturating_sub(size_of::<Authority>())
        .saturating_add(2 * MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_RETAINED_BYTES)
        .saturating_add(2 * RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
        .saturating_add(TEMPORARY_MANIFEST_HANDLE_BYTES)
}

const fn applied_additional_retained_bytes<Authority>() -> usize {
    size_of::<JournalActivationApplied<Authority>>()
        .saturating_sub(size_of::<Authority>())
        .saturating_add(size_of::<ExtensionNativeOwnershipEntry>())
        .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
}

const fn maximum_applied_additional_retained_bytes<Authority>() -> usize {
    applied_additional_retained_bytes::<Authority>().saturating_add(TEMPORARY_MANIFEST_HANDLE_BYTES)
}

/// Maximum additional adapter charge reserved before one activation call.
/// This covers its temporary Store manifest handle and the worst retained
/// reconciliation state. The already-admitted nominal authority and shared
/// manifest pointee are intentionally excluded.
pub(crate) const MAX_JOURNAL_ACTIVATION_AMBIGUITY_ADDITIONAL_RETAINED_BYTES: usize =
    if ambiguity_additional_retained_bytes::<ServiceRuntimeAcquisitionPlan>()
        > ambiguity_additional_retained_bytes::<ServiceRuntimePackageAccess>()
    {
        ambiguity_additional_retained_bytes::<ServiceRuntimeAcquisitionPlan>()
    } else {
        ambiguity_additional_retained_bytes::<ServiceRuntimePackageAccess>()
    };

const _: () = assert!(
    MAX_JOURNAL_ACTIVATION_AMBIGUITY_ADDITIONAL_RETAINED_BYTES
        >= maximum_applied_additional_retained_bytes::<ServiceRuntimeAcquisitionPlan>()
);
const _: () = assert!(
    MAX_JOURNAL_ACTIVATION_AMBIGUITY_ADDITIONAL_RETAINED_BYTES
        >= maximum_applied_additional_retained_bytes::<ServiceRuntimePackageAccess>()
);
const _: () = assert!(
    MAX_JOURNAL_ACTIVATION_AMBIGUITY_ADDITIONAL_RETAINED_BYTES
        < MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
);

/// Store calls that atomically revalidate the install/grant cohort with one
/// fresh native-ownership journal mutation.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) trait FencedActivationJournalBackend: JournalBackend {
    fn begin_until(
        &self,
        expected: zephium_core::extensions::ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipActivationOutcome>;

    fn transition_to_may_own_until(
        &self,
        expected: zephium_core::extensions::ExtensionNativeOwnershipJournalRevision,
        preparing: ExtensionNativeOwnershipEntryCas,
        expected_native_identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipActivationOutcome>;
}

impl FencedActivationJournalBackend for ExtensionServiceStoreAuthority {
    fn begin_until(
        &self,
        expected: zephium_core::extensions::ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipActivationOutcome> {
        self.begin_native_ownership_until(expected, mutation, manifest, deadline)
    }

    fn transition_to_may_own_until(
        &self,
        expected: zephium_core::extensions::ExtensionNativeOwnershipJournalRevision,
        preparing: ExtensionNativeOwnershipEntryCas,
        expected_native_identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipActivationOutcome> {
        self.transition_native_ownership_to_may_own_until(
            expected,
            preparing,
            expected_native_identity,
            manifest,
            deadline,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum JournalActivationPreparationFailureReason {
    ReloadRequired,
    RetainedBytesOverflow,
    RetainedBytesExceeded,
    PackageAccessEntryMismatch,
    ExpectedNativeIdentityUnavailable(BundledRuntimePackageAccessBuildError),
    MutationKindMismatch,
    ManifestPackageMismatch,
    CurrentRowMismatch,
    InvalidIdentityDirection,
    InvalidLocalTransition,
}

/// A pre-Store refusal which returns the caller's exact nominal authority.
#[must_use = "activation preparation authority must be retained or deliberately settled"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct JournalActivationPreparationFailure<Authority> {
    reason: JournalActivationPreparationFailureReason,
    authority: Authority,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<Authority> JournalActivationPreparationFailure<Authority> {
    pub(crate) const fn reason(&self) -> JournalActivationPreparationFailureReason {
        self.reason
    }

    pub(crate) fn into_authority(self) -> Authority {
        self.authority
    }
}

/// Definite Store refusal after the fenced cohort check. These values contain
/// no package, path, profile, install, or digest payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum JournalActivationRefusalReason {
    NotRegistered,
    DegradedProfile,
    SessionRecoveryRequired,
    Stale(ExtensionNativeOwnershipActivationStale),
    EligibilityChanged(ExtensionRuntimeEligibilityDenial),
    Invalid,
    LimitReached,
    RevisionExhausted,
    Failed,
}

/// Lossless authority returned after definite non-application.
#[must_use = "returned activation authority must be retained or deliberately settled"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct JournalActivationReturned<Authority> {
    authority: Authority,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<Authority> JournalActivationReturned<Authority> {
    pub(crate) fn into_authority(self) -> Authority {
        self.authority
    }
}

/// Exact locally predicted row after a Store-confirmed fenced mutation.
#[must_use = "applied activation authority and row must be settled together"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct JournalActivationApplied<Authority> {
    returned: JournalActivationReturned<Authority>,
    entry: Box<ExtensionNativeOwnershipEntry>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<Authority> JournalActivationApplied<Authority> {
    pub(crate) const fn entry(&self) -> &ExtensionNativeOwnershipEntry {
        &self.entry
    }

    /// Adapter-owned charge beyond the already-admitted nominal authority and
    /// shared manifest pointee.
    pub(crate) const fn additional_retained_bytes(&self) -> usize {
        applied_additional_retained_bytes::<Authority>()
    }

    pub(crate) fn into_parts(self) -> (Authority, ExtensionNativeOwnershipEntry) {
        (self.returned.into_authority(), *self.entry)
    }
}

#[must_use = "refused activation authority must be retained or deliberately settled"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct JournalActivationRefused<Authority> {
    reason: JournalActivationRefusalReason,
    returned: JournalActivationReturned<Authority>,
}

/// Definite pre-commit conflict. Store attempted no journal mutation, but the
/// local projection is invalidated before returning authority so a fresh
/// transaction cannot be admitted until a complete reload.
#[must_use = "conflicted activation authority requires a fresh journal reload"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct JournalActivationConflict<Authority> {
    current: zephium_core::extensions::ExtensionNativeOwnershipJournalRevision,
    returned: JournalActivationReturned<Authority>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<Authority> JournalActivationConflict<Authority> {
    pub(crate) const fn current(
        &self,
    ) -> zephium_core::extensions::ExtensionNativeOwnershipJournalRevision {
        self.current
    }

    pub(crate) fn into_returned(self) -> JournalActivationReturned<Authority> {
        self.returned
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl<Authority> JournalActivationRefused<Authority> {
    pub(crate) const fn reason(&self) -> JournalActivationRefusalReason {
        self.reason
    }

    pub(crate) fn into_returned(self) -> JournalActivationReturned<Authority> {
        self.returned
    }
}

/// An admitted call whose exact durable settlement is unknown. Authority is
/// captive until a complete journal reload proves an exact frontier.
#[must_use = "ambiguous activation authority must remain captive until reconciliation"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct JournalActivationReloadRequired<Authority> {
    returned: JournalActivationReturned<Authority>,
    before: ExtensionNativeOwnershipJournal,
    after: ExtensionNativeOwnershipJournal,
    affected: ExtensionNativeOwnershipKey,
}

/// Known third-frontier reconciliation result. No activation authority is
/// exposed because neither definite application nor definite non-application
/// follows from the reloaded durable cohort.
#[must_use = "divergent activation authority is a fail-stop obligation"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct JournalActivationDiverged<Authority> {
    _unsettled: JournalActivationReloadRequired<Authority>,
}

/// Exact observation after a complete post-ambiguity journal reload.
#[must_use = "activation reconciliation authority must be settled exactly once"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum JournalActivationReconciliation<Authority> {
    Applied(JournalActivationApplied<Authority>),
    NotApplied(JournalActivationReturned<Authority>),
    ReloadPending(JournalActivationReloadRequired<Authority>),
    Diverged(JournalActivationDiverged<Authority>),
}

#[cfg_attr(not(test), allow(dead_code))]
impl<Authority> JournalActivationReloadRequired<Authority> {
    /// Maximum adapter-owned charge which must be reserved before admitting
    /// Store work that could produce this ambiguity wrapper.
    pub(crate) const fn maximum_additional_retained_bytes() -> usize {
        ambiguity_additional_retained_bytes::<Authority>()
    }

    /// Actual adapter-owned charge, excluding the already-accounted nominal
    /// authority and shared manifest pointee.
    pub(crate) fn additional_retained_bytes(&self) -> usize {
        size_of::<JournalActivationReloadRequired<Authority>>()
            .saturating_sub(size_of::<Authority>())
            .saturating_add(self.before.retained_bytes())
            .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
            .saturating_add(self.after.retained_bytes())
            .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
    }

    /// Recovers authority only after a complete projection proves the exact
    /// before or predicted-after journal. Any third frontier stays captive.
    pub(crate) fn reconcile(
        self,
        projection: &JournalProjection,
    ) -> JournalActivationReconciliation<Authority> {
        let Some(journal) = projection.known() else {
            return JournalActivationReconciliation::ReloadPending(self);
        };
        if journal.exactly_matches_durable_state(&self.after) {
            let entry = self
                .after
                .get(self.affected)
                .expect("predicted fresh activation journal must retain its affected row")
                .clone();
            return JournalActivationReconciliation::Applied(JournalActivationApplied {
                returned: self.returned,
                entry: Box::new(entry),
            });
        }
        if journal.exactly_matches_durable_state(&self.before) {
            JournalActivationReconciliation::NotApplied(self.returned)
        } else {
            JournalActivationReconciliation::Diverged(JournalActivationDiverged {
                _unsettled: self,
            })
        }
    }
}

#[must_use = "activation settlement authority must be handled exactly once"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum JournalActivationSettlement<Authority> {
    Applied(JournalActivationApplied<Authority>),
    NotAdmitted(JournalActivationReturned<Authority>),
    Refused(JournalActivationRefused<Authority>),
    Conflict(JournalActivationConflict<Authority>),
    ReloadRequired(JournalActivationReloadRequired<Authority>),
}

impl JournalProjection {
    /// Settles one repository-derived Begin while preserving the exact
    /// same-open plan wrapper and its open-epoch guard.
    #[allow(clippy::result_large_err)] // A pre-Store refusal returns the move-only plan allocation-free.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn settle_fenced_begin(
        &mut self,
        backend: &impl FencedActivationJournalBackend,
        plan: ServiceRuntimeAcquisitionPlan,
        deadline: Instant,
    ) -> Result<
        JournalActivationSettlement<ServiceRuntimeAcquisitionPlan>,
        JournalActivationPreparationFailure<ServiceRuntimeAcquisitionPlan>,
    > {
        if let Some(reason) =
            retained_budget_failure::<ServiceRuntimeAcquisitionPlan>(plan.retained_bytes())
        {
            return Err(preparation_failure(reason, plan));
        }
        let mutation = plan.ownership_begin_mutation();
        let manifest = Arc::clone(plan.manifest());
        self.settle_fenced_begin_with_authority(backend, plan, mutation, manifest, deadline)
    }

    fn settle_fenced_begin_with_authority<Authority>(
        &mut self,
        backend: &impl FencedActivationJournalBackend,
        authority: Authority,
        mutation: ExtensionNativeOwnershipJournalMutation,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> Result<
        JournalActivationSettlement<Authority>,
        JournalActivationPreparationFailure<Authority>,
    > {
        let Some(current) = self.journal.as_ref() else {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::ReloadRequired,
                authority,
            ));
        };
        let ExtensionNativeOwnershipJournalMutation::Begin(preparation) = &mutation else {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::MutationKindMismatch,
                authority,
            ));
        };
        if manifest.package() != preparation.package() {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::ManifestPackageMismatch,
                authority,
            ));
        }
        if current.get(preparation.key()).is_some() {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::CurrentRowMismatch,
                authority,
            ));
        }
        let application = match current.clone().apply(current.revision(), mutation.clone()) {
            Ok(application) => application,
            Err(_) => {
                return Err(preparation_failure(
                    JournalActivationPreparationFailureReason::InvalidLocalTransition,
                    authority,
                ));
            }
        };
        let outcome = backend.begin_until(current.revision(), mutation, manifest, deadline);
        Ok(self.settle_fenced_activation(authority, application, outcome))
    }

    /// Settles one repository-authenticated Preparing-to-MayOwn transaction.
    ///
    /// The manifest and expected native identity are derived from the exact
    /// same-open package access. Callers cannot substitute either value.
    #[allow(clippy::result_large_err)] // A pre-Store refusal returns same-open access allocation-free.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn settle_preparing_to_may_own(
        &mut self,
        backend: &impl FencedActivationJournalBackend,
        access: ServiceRuntimePackageAccess,
        preparing: ExtensionNativeOwnershipEntry,
        deadline: Instant,
    ) -> Result<
        JournalActivationSettlement<ServiceRuntimePackageAccess>,
        JournalActivationPreparationFailure<ServiceRuntimePackageAccess>,
    > {
        if let Some(reason) =
            retained_budget_failure::<ServiceRuntimePackageAccess>(access.retained_bytes())
        {
            return Err(preparation_failure(reason, access));
        }
        if !access.matches_preparing_ownership_entry(&preparing) {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::PackageAccessEntryMismatch,
                access,
            ));
        }
        let expected_native_identity = match access.expected_native_identity() {
            Ok(identity) => identity,
            Err(error) => {
                return Err(preparation_failure(
                    JournalActivationPreparationFailureReason::ExpectedNativeIdentityUnavailable(
                        error,
                    ),
                    access,
                ));
            }
        };
        let manifest = Arc::clone(access.manifest());
        self.settle_preparing_to_may_own_with_authority(
            backend,
            access,
            preparing,
            expected_native_identity,
            manifest,
            deadline,
        )
    }

    fn settle_preparing_to_may_own_with_authority<Authority>(
        &mut self,
        backend: &impl FencedActivationJournalBackend,
        authority: Authority,
        preparing: ExtensionNativeOwnershipEntry,
        expected_native_identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> Result<
        JournalActivationSettlement<Authority>,
        JournalActivationPreparationFailure<Authority>,
    > {
        let Some(current) = self.journal.as_ref() else {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::ReloadRequired,
                authority,
            ));
        };
        if current.get(preparing.key()) != Some(&preparing)
            || preparing.intent() != ExtensionNativeOwnershipIntent::Acquire
            || preparing.phase() != ExtensionNativeOwnershipPhase::NativeAbsentPreparing
            || preparing.expected_native_identity().is_some()
            || preparing.native_identity().is_some()
        {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::CurrentRowMismatch,
                authority,
            ));
        }
        if manifest.package() != preparing.package() {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::ManifestPackageMismatch,
                authority,
            ));
        }
        let native_backend = matches!(
            preparing.runtime_backend(),
            ExtensionRuntimeBackendTarget::MacosNative
                | ExtensionRuntimeBackendTarget::WindowsNative
        );
        if native_backend != expected_native_identity.is_some()
            || expected_native_identity
                .is_some_and(|identity| identity.backend() != preparing.runtime_backend())
        {
            return Err(preparation_failure(
                JournalActivationPreparationFailureReason::InvalidIdentityDirection,
                authority,
            ));
        }
        let preparing_cas = preparing.cas();
        let mutation = expected_native_identity.map_or_else(
            || {
                ExtensionNativeOwnershipJournalMutation::transition(
                    preparing_cas,
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                )
            },
            |identity| {
                ExtensionNativeOwnershipJournalMutation::transition_with_expected_native_identity(
                    preparing_cas,
                    identity,
                )
            },
        );
        let application = match current.clone().apply(current.revision(), mutation) {
            Ok(application) => application,
            Err(_) => {
                return Err(preparation_failure(
                    JournalActivationPreparationFailureReason::InvalidLocalTransition,
                    authority,
                ));
            }
        };
        let outcome = backend.transition_to_may_own_until(
            current.revision(),
            preparing_cas,
            expected_native_identity,
            manifest,
            deadline,
        );
        Ok(self.settle_fenced_activation(authority, application, outcome))
    }

    fn settle_fenced_activation<Authority>(
        &mut self,
        authority: Authority,
        application: zephium_core::extensions::ExtensionNativeOwnershipJournalApplication,
        outcome: ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipActivationOutcome>,
    ) -> JournalActivationSettlement<Authority> {
        let current = self
            .journal
            .take()
            .expect("checked fenced activation requires a known projection");
        let applied_entry = application
            .entry()
            .expect("fresh activation must produce one row")
            .clone();
        let affected = applied_entry.key();
        let returned = JournalActivationReturned { authority };
        match outcome {
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipActivationOutcome::Applied(applied),
            ) if applied_matches(&application, &applied) => {
                self.journal = Some(application.into_journal());
                JournalActivationSettlement::Applied(JournalActivationApplied {
                    returned,
                    entry: Box::new(applied_entry),
                })
            }
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipActivationOutcome::Conflict { current },
            ) => JournalActivationSettlement::Conflict(JournalActivationConflict {
                current,
                returned,
            }),
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipActivationOutcome::Applied(_)
                | ExtensionNativeOwnershipActivationOutcome::OutcomeUnknown,
            )
            | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
                JournalActivationSettlement::ReloadRequired(JournalActivationReloadRequired {
                    returned,
                    before: current,
                    after: application.into_journal(),
                    affected,
                })
            }
            ExtensionServiceStoreCallOutcome::NotAdmitted => {
                self.journal = Some(current);
                JournalActivationSettlement::NotAdmitted(returned)
            }
            ExtensionServiceStoreCallOutcome::Completed(outcome) => {
                self.journal = Some(current);
                let reason = refusal_reason(outcome)
                    .expect("all non-ambiguous activation outcomes must be definite refusals");
                JournalActivationSettlement::Refused(JournalActivationRefused { reason, returned })
            }
        }
    }
}

fn preparation_failure<Authority>(
    reason: JournalActivationPreparationFailureReason,
    authority: Authority,
) -> JournalActivationPreparationFailure<Authority> {
    JournalActivationPreparationFailure { reason, authority }
}

fn retained_budget_failure<Authority>(
    authority_retained_bytes: usize,
) -> Option<JournalActivationPreparationFailureReason> {
    match authority_retained_bytes.checked_add(ambiguity_additional_retained_bytes::<Authority>()) {
        None => Some(JournalActivationPreparationFailureReason::RetainedBytesOverflow),
        Some(total) if total > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => {
            Some(JournalActivationPreparationFailureReason::RetainedBytesExceeded)
        }
        Some(_) => None,
    }
}

fn refusal_reason(
    outcome: ExtensionNativeOwnershipActivationOutcome,
) -> Option<JournalActivationRefusalReason> {
    match outcome {
        ExtensionNativeOwnershipActivationOutcome::NotRegistered => {
            Some(JournalActivationRefusalReason::NotRegistered)
        }
        ExtensionNativeOwnershipActivationOutcome::DegradedProfile => {
            Some(JournalActivationRefusalReason::DegradedProfile)
        }
        ExtensionNativeOwnershipActivationOutcome::SessionRecoveryRequired => {
            Some(JournalActivationRefusalReason::SessionRecoveryRequired)
        }
        ExtensionNativeOwnershipActivationOutcome::Stale(reason) => {
            Some(JournalActivationRefusalReason::Stale(reason))
        }
        ExtensionNativeOwnershipActivationOutcome::EligibilityChanged(reason) => {
            Some(JournalActivationRefusalReason::EligibilityChanged(reason))
        }
        ExtensionNativeOwnershipActivationOutcome::Invalid => {
            Some(JournalActivationRefusalReason::Invalid)
        }
        ExtensionNativeOwnershipActivationOutcome::LimitReached => {
            Some(JournalActivationRefusalReason::LimitReached)
        }
        ExtensionNativeOwnershipActivationOutcome::RevisionExhausted => {
            Some(JournalActivationRefusalReason::RevisionExhausted)
        }
        ExtensionNativeOwnershipActivationOutcome::Failed => {
            Some(JournalActivationRefusalReason::Failed)
        }
        ExtensionNativeOwnershipActivationOutcome::Applied(_)
        | ExtensionNativeOwnershipActivationOutcome::Conflict { .. }
        | ExtensionNativeOwnershipActivationOutcome::OutcomeUnknown => None,
    }
}

#[cfg(test)]
mod tests;
