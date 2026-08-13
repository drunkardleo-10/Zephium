//! Bounded exact-row fences for post-activation ownership transitions.

use std::mem::size_of;
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipIdentity,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipJournal,
    ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipJournalRevision,
    ExtensionNativeOwnershipOperation, ExtensionNativeOwnershipPhase,
};
use zephium_core::ports::store::ExtensionNativeOwnershipJournalMutationOutcome;
use zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES;
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::{applied_matches, JournalBackend, JournalProjection};

const RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();
const MAX_RETAINED_ROWS: usize = 2;
const MAX_RETAINED_ROW_BYTES: usize = MAX_RETAINED_ROWS
    * (size_of::<ExtensionNativeOwnershipEntry>() + RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES);

const fn maximum_inline_bytes() -> usize {
    let settlement = size_of::<RowFenceSettlement>();
    let reconciliation = size_of::<RowFenceReconciliation>();
    if settlement > reconciliation {
        settlement
    } else {
        reconciliation
    }
}

/// Exact maximum adapter charge reserved before admitting one row mutation.
/// Caller-owned runtime authority is excluded and remains outside the fence.
pub(crate) const MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES: usize =
    maximum_inline_bytes() + MAX_RETAINED_ROW_BYTES;

const _: () =
    assert!(MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES < MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RowFencePreparationFailureReason {
    ReloadRequired,
    RetainedBytesOverflow,
    RetainedBytesExceeded,
    CurrentRowMismatch,
    ForbiddenLifecycleEdge,
    InvalidLocalTransition,
}

/// Store-free refusal. Caller authority and the borrowed row remain external.
#[must_use = "row-fence preparation failure must be handled"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RowFencePreparationFailure {
    reason: RowFencePreparationFailureReason,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RowFencePreparationFailure {
    pub(crate) const fn reason(&self) -> RowFencePreparationFailureReason {
        self.reason
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RowFenceRefusalReason {
    NotRegistered,
    DegradedProfile,
    SessionRecoveryRequired,
    Invalid,
    LimitReached,
    RevisionExhausted,
    Failed,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct RowFenceClocks {
    revision: ExtensionNativeOwnershipJournalRevision,
    operation_high_water: Option<ExtensionNativeOwnershipOperation>,
    native_incarnation_high_water: Option<ExtensionNativeIncarnation>,
}

impl RowFenceClocks {
    const fn from_journal(journal: &ExtensionNativeOwnershipJournal) -> Self {
        Self {
            revision: journal.revision(),
            operation_high_water: journal.operation_high_water(),
            native_incarnation_high_water: journal.native_incarnation_high_water(),
        }
    }

    fn matches(self, journal: &ExtensionNativeOwnershipJournal) -> bool {
        self == Self::from_journal(journal)
    }
}

struct RowFenceFrontier {
    clocks: RowFenceClocks,
    row: Option<Box<ExtensionNativeOwnershipEntry>>,
}

impl RowFenceFrontier {
    fn capture(
        journal: &ExtensionNativeOwnershipJournal,
        key: zephium_core::extensions::ExtensionNativeOwnershipKey,
    ) -> Self {
        Self {
            clocks: RowFenceClocks::from_journal(journal),
            row: journal.get(key).cloned().map(Box::new),
        }
    }

    fn row(&self) -> Option<&ExtensionNativeOwnershipEntry> {
        self.row.as_deref()
    }

    fn retained_row_count(&self) -> usize {
        usize::from(self.row.is_some())
    }

    /// Exact affected-row plus global-clock proof. Because one serialized
    /// coordinator is the sole writer and halts mutation after ambiguity, an
    /// exact global revision/high-water tuple identifies the unique commit
    /// frontier; an unrelated row cannot change at that same frontier.
    fn matches(
        &self,
        journal: &ExtensionNativeOwnershipJournal,
        key: zephium_core::extensions::ExtensionNativeOwnershipKey,
    ) -> bool {
        self.clocks.matches(journal) && journal.get(key) == self.row()
    }
}

#[must_use = "applied row evidence must be consumed by the coordinator"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RowFenceApplied {
    after: RowFenceFrontier,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RowFenceApplied {
    pub(crate) fn entry(&self) -> Option<&ExtensionNativeOwnershipEntry> {
        self.after.row()
    }

    pub(crate) const fn journal_revision(&self) -> ExtensionNativeOwnershipJournalRevision {
        self.after.clocks.revision
    }
}

#[must_use = "definite row-mutation refusal must be handled"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RowFenceRefused {
    reason: RowFenceRefusalReason,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RowFenceRefused {
    pub(crate) const fn reason(&self) -> RowFenceRefusalReason {
        self.reason
    }
}

/// Definite non-application with the exact expected-before evidence retained
/// for inspection after the invalidated projection is fully reloaded.
#[must_use = "conflict evidence requires a complete journal reload"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RowFenceConflict {
    reported_current: ExtensionNativeOwnershipJournalRevision,
    before: RowFenceFrontier,
}

/// Borrowed inspection of a complete post-conflict reload. Neither variant
/// claims that the refused mutation was applied.
#[must_use = "post-conflict inspection must be handled"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RowFenceConflictInspection<'journal> {
    Pending,
    Reloaded {
        reported_revision_matches: bool,
        expected_before_matches: bool,
        current: Option<&'journal ExtensionNativeOwnershipEntry>,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
impl RowFenceConflict {
    pub(crate) const fn reported_current(&self) -> ExtensionNativeOwnershipJournalRevision {
        self.reported_current
    }

    pub(crate) fn expected_before(&self) -> &ExtensionNativeOwnershipEntry {
        self.before
            .row()
            .expect("row mutation conflict always retains its expected row")
    }

    pub(crate) fn inspect<'journal>(
        &self,
        projection: &'journal JournalProjection,
    ) -> RowFenceConflictInspection<'journal> {
        let Some(journal) = projection.known() else {
            return RowFenceConflictInspection::Pending;
        };
        let key = self.expected_before().key();
        RowFenceConflictInspection::Reloaded {
            reported_revision_matches: journal.revision() == self.reported_current,
            expected_before_matches: self.before.matches(journal, key),
            current: journal.get(key),
        }
    }
}

/// True commit ambiguity retaining only bounded before/after row-and-clock
/// evidence. It never owns runtime or repository authority.
#[must_use = "ambiguous row mutation must be reconciled after a complete reload"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RowFenceReloadRequired {
    before: RowFenceFrontier,
    after: RowFenceFrontier,
}

#[must_use = "definite non-application evidence must be consumed"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RowFenceNotApplied {
    before: RowFenceFrontier,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RowFenceNotApplied {
    pub(crate) fn entry(&self) -> &ExtensionNativeOwnershipEntry {
        self.before
            .row()
            .expect("row mutation before frontier always contains its row")
    }
}

#[must_use = "divergent row evidence is a fail-stop obligation"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RowFenceDiverged {
    _unsettled: RowFenceReloadRequired,
}

#[must_use = "row-fence reconciliation must be handled exactly once"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RowFenceReconciliation {
    Applied(RowFenceApplied),
    NotApplied(RowFenceNotApplied),
    Pending(RowFenceReloadRequired),
    Diverged(RowFenceDiverged),
}

#[cfg_attr(not(test), allow(dead_code))]
impl RowFenceReconciliation {
    pub(crate) fn additional_retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.retained_row_count()
                    .saturating_mul(size_of::<ExtensionNativeOwnershipEntry>()),
            )
            .saturating_add(
                self.retained_row_count()
                    .saturating_mul(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES),
            )
    }

    fn retained_row_count(&self) -> usize {
        match self {
            Self::Applied(applied) => applied.after.retained_row_count(),
            Self::NotApplied(not_applied) => not_applied.before.retained_row_count(),
            Self::Pending(pending) => pending.retained_row_count(),
            Self::Diverged(diverged) => diverged._unsettled.retained_row_count(),
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RowFenceReloadRequired {
    pub(crate) const fn maximum_additional_retained_bytes() -> usize {
        MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES
    }

    pub(crate) fn additional_retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.retained_row_count()
                    .saturating_mul(size_of::<ExtensionNativeOwnershipEntry>()),
            )
            .saturating_add(
                self.retained_row_count()
                    .saturating_mul(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES),
            )
    }

    fn retained_row_count(&self) -> usize {
        self.before.retained_row_count() + self.after.retained_row_count()
    }

    pub(crate) fn reconcile(self, projection: &JournalProjection) -> RowFenceReconciliation {
        let Some(journal) = projection.known() else {
            return RowFenceReconciliation::Pending(self);
        };
        let key = self
            .before
            .row()
            .expect("row mutation before frontier always contains its row")
            .key();
        if self.after.matches(journal, key) {
            return RowFenceReconciliation::Applied(RowFenceApplied { after: self.after });
        }
        if self.before.matches(journal, key) {
            RowFenceReconciliation::NotApplied(RowFenceNotApplied {
                before: self.before,
            })
        } else {
            RowFenceReconciliation::Diverged(RowFenceDiverged { _unsettled: self })
        }
    }
}

#[must_use = "row-fence settlement must be handled exactly once"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RowFenceSettlement {
    Applied(RowFenceApplied),
    NotAdmitted,
    Refused(RowFenceRefused),
    Conflict(RowFenceConflict),
    ReloadRequired(RowFenceReloadRequired),
}

#[cfg_attr(not(test), allow(dead_code))]
impl RowFenceSettlement {
    pub(crate) fn additional_retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.retained_row_count()
                    .saturating_mul(size_of::<ExtensionNativeOwnershipEntry>()),
            )
            .saturating_add(
                self.retained_row_count()
                    .saturating_mul(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES),
            )
    }

    fn retained_row_count(&self) -> usize {
        match self {
            Self::Applied(applied) => applied.after.retained_row_count(),
            Self::NotAdmitted | Self::Refused(_) => 0,
            Self::Conflict(conflict) => conflict.before.retained_row_count(),
            Self::ReloadRequired(pending) => pending.retained_row_count(),
        }
    }
}

impl JournalProjection {
    /// Applies one exact allowed lifecycle edge for the current durable row.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn settle_exact_row_transition(
        &mut self,
        backend: &impl JournalBackend,
        current: &ExtensionNativeOwnershipEntry,
        next_intent: ExtensionNativeOwnershipIntent,
        next_phase: ExtensionNativeOwnershipPhase,
        caller_retained_bytes: usize,
        deadline: Instant,
    ) -> Result<RowFenceSettlement, RowFencePreparationFailure> {
        preflight_retained_bytes(caller_retained_bytes)?;
        if !allowed_row_transition(current, next_intent, next_phase) {
            return Err(preparation_failure(
                RowFencePreparationFailureReason::ForbiddenLifecycleEdge,
            ));
        }
        let mutation = ExtensionNativeOwnershipJournalMutation::transition(
            current.cas(),
            next_intent,
            next_phase,
        );
        self.settle_exact_row_mutation(backend, current, mutation, deadline)
    }

    /// Attaches one observed native identity only at Acquire/NativeMayOwn.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn settle_native_identity_attachment(
        &mut self,
        backend: &impl JournalBackend,
        current: &ExtensionNativeOwnershipEntry,
        native_identity: ExtensionNativeOwnershipIdentity,
        caller_retained_bytes: usize,
        deadline: Instant,
    ) -> Result<RowFenceSettlement, RowFencePreparationFailure> {
        preflight_retained_bytes(caller_retained_bytes)?;
        if current.intent() != ExtensionNativeOwnershipIntent::Acquire
            || current.phase() != ExtensionNativeOwnershipPhase::NativeMayOwn
        {
            return Err(preparation_failure(
                RowFencePreparationFailureReason::ForbiddenLifecycleEdge,
            ));
        }
        let mutation = ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
            current.cas(),
            current.intent(),
            current.phase(),
            native_identity,
        );
        self.settle_exact_row_mutation(backend, current, mutation, deadline)
    }

    /// Clears only an exact Release/NativeAbsentReleasePending row.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn settle_exact_row_clear(
        &mut self,
        backend: &impl JournalBackend,
        current: &ExtensionNativeOwnershipEntry,
        caller_retained_bytes: usize,
        deadline: Instant,
    ) -> Result<RowFenceSettlement, RowFencePreparationFailure> {
        preflight_retained_bytes(caller_retained_bytes)?;
        if current.intent() != ExtensionNativeOwnershipIntent::Release
            || current.phase() != ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        {
            return Err(preparation_failure(
                RowFencePreparationFailureReason::ForbiddenLifecycleEdge,
            ));
        }
        self.settle_exact_row_mutation(
            backend,
            current,
            ExtensionNativeOwnershipJournalMutation::clear(current.cas()),
            deadline,
        )
    }

    fn settle_exact_row_mutation(
        &mut self,
        backend: &impl JournalBackend,
        expected_row: &ExtensionNativeOwnershipEntry,
        mutation: ExtensionNativeOwnershipJournalMutation,
        deadline: Instant,
    ) -> Result<RowFenceSettlement, RowFencePreparationFailure> {
        let Some(current) = self.journal.as_ref() else {
            return Err(preparation_failure(
                RowFencePreparationFailureReason::ReloadRequired,
            ));
        };
        if current.get(expected_row.key()) != Some(expected_row) {
            return Err(preparation_failure(
                RowFencePreparationFailureReason::CurrentRowMismatch,
            ));
        }
        let application = current
            .clone()
            .apply(current.revision(), mutation.clone())
            .map_err(|_| {
                preparation_failure(RowFencePreparationFailureReason::InvalidLocalTransition)
            })?;
        let outcome = backend.mutate_until(current, mutation, deadline);
        let current = self
            .journal
            .take()
            .expect("checked row mutation requires a known projection");
        let key = expected_row.key();
        Ok(match outcome {
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied),
            ) if applied_matches(&application, &applied) => {
                let after = RowFenceFrontier::capture(application.journal(), key);
                self.journal = Some(application.into_journal());
                RowFenceSettlement::Applied(RowFenceApplied { after })
            }
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Conflict {
                    current: reported_current,
                },
            ) => RowFenceSettlement::Conflict(RowFenceConflict {
                reported_current,
                before: RowFenceFrontier::capture(&current, key),
            }),
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Applied(_)
                | ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown,
            )
            | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
                RowFenceSettlement::ReloadRequired(RowFenceReloadRequired {
                    before: RowFenceFrontier::capture(&current, key),
                    after: RowFenceFrontier::capture(application.journal(), key),
                })
            }
            ExtensionServiceStoreCallOutcome::NotAdmitted => {
                self.journal = Some(current);
                RowFenceSettlement::NotAdmitted
            }
            ExtensionServiceStoreCallOutcome::Completed(outcome) => {
                self.journal = Some(current);
                RowFenceSettlement::Refused(RowFenceRefused {
                    reason: refusal_reason(outcome)
                        .expect("remaining journal outcomes are definite refusals"),
                })
            }
        })
    }
}

fn preflight_retained_bytes(
    caller_retained_bytes: usize,
) -> Result<(), RowFencePreparationFailure> {
    match caller_retained_bytes.checked_add(MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES) {
        None => Err(preparation_failure(
            RowFencePreparationFailureReason::RetainedBytesOverflow,
        )),
        Some(total) if total > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => Err(
            preparation_failure(RowFencePreparationFailureReason::RetainedBytesExceeded),
        ),
        Some(_) => Ok(()),
    }
}

const fn preparation_failure(
    reason: RowFencePreparationFailureReason,
) -> RowFencePreparationFailure {
    RowFencePreparationFailure { reason }
}

const fn allowed_row_transition(
    current: &ExtensionNativeOwnershipEntry,
    next_intent: ExtensionNativeOwnershipIntent,
    next_phase: ExtensionNativeOwnershipPhase,
) -> bool {
    use ExtensionNativeOwnershipIntent::{Acquire, Release};
    use ExtensionNativeOwnershipPhase::{
        NativeAbsentPreparing, NativeAbsentReleasePending, NativeMayOwn, NativeOwned,
    };
    matches!(
        (current.intent(), current.phase(), next_intent, next_phase),
        (Acquire, NativeMayOwn, Acquire, NativeOwned)
            | (Acquire, NativeMayOwn, Release, NativeMayOwn)
            | (
                Acquire,
                NativeAbsentPreparing | NativeMayOwn,
                Release,
                NativeAbsentReleasePending
            )
            | (Acquire, NativeOwned, Release, NativeMayOwn)
            | (Release, NativeMayOwn, Release, NativeAbsentReleasePending)
    )
}

pub(super) fn requires_fenced_activation(
    journal: &ExtensionNativeOwnershipJournal,
    mutation: &ExtensionNativeOwnershipJournalMutation,
) -> bool {
    match mutation {
        ExtensionNativeOwnershipJournalMutation::Begin(_) => true,
        ExtensionNativeOwnershipJournalMutation::Transition {
            expected,
            intent: ExtensionNativeOwnershipIntent::Acquire,
            phase: ExtensionNativeOwnershipPhase::NativeMayOwn,
            ..
        } => journal.get(expected.key()).is_some_and(|current| {
            current.intent() == ExtensionNativeOwnershipIntent::Acquire
                && current.phase() == ExtensionNativeOwnershipPhase::NativeAbsentPreparing
        }),
        ExtensionNativeOwnershipJournalMutation::Transition { .. }
        | ExtensionNativeOwnershipJournalMutation::RebindGrants { .. }
        | ExtensionNativeOwnershipJournalMutation::Clear { .. } => false,
    }
}

fn refusal_reason(
    outcome: ExtensionNativeOwnershipJournalMutationOutcome,
) -> Option<RowFenceRefusalReason> {
    match outcome {
        ExtensionNativeOwnershipJournalMutationOutcome::NotRegistered => {
            Some(RowFenceRefusalReason::NotRegistered)
        }
        ExtensionNativeOwnershipJournalMutationOutcome::DegradedProfile => {
            Some(RowFenceRefusalReason::DegradedProfile)
        }
        ExtensionNativeOwnershipJournalMutationOutcome::SessionRecoveryRequired => {
            Some(RowFenceRefusalReason::SessionRecoveryRequired)
        }
        ExtensionNativeOwnershipJournalMutationOutcome::Invalid => {
            Some(RowFenceRefusalReason::Invalid)
        }
        ExtensionNativeOwnershipJournalMutationOutcome::LimitReached => {
            Some(RowFenceRefusalReason::LimitReached)
        }
        ExtensionNativeOwnershipJournalMutationOutcome::RevisionExhausted => {
            Some(RowFenceRefusalReason::RevisionExhausted)
        }
        ExtensionNativeOwnershipJournalMutationOutcome::Failed => {
            Some(RowFenceRefusalReason::Failed)
        }
        ExtensionNativeOwnershipJournalMutationOutcome::Applied(_)
        | ExtensionNativeOwnershipJournalMutationOutcome::Conflict { .. }
        | ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown => None,
    }
}

#[cfg(test)]
mod tests;
