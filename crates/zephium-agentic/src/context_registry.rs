//! Bounded single-owner registry for agent-browser contexts.
//!
//! The application actor owns this registry. It has no locks, threads,
//! timers, native objects, or I/O. Queued rows consume only bounded Rust data;
//! an executing permit is reserved before construction, resume, or renderer
//! recovery can be requested.

use std::collections::BTreeMap;

use thiserror::Error;
use zephium_core::ids::ProfileId;

use crate::context::ContextRecord;
use crate::{
    ContextCapabilities, ContextId, ContextIdentity, ContextJoin, ContextKind, ContextLifecycle,
    ContextOperationId, ContextOperationJoin, ContextRunId, ContextSettlement, ContextStatus,
    ContextTerminal, ContextTransitionError,
};

/// Initial hard ceiling for logical agent contexts in one process.
pub const MAX_LIVE_CONTEXTS: usize = 8;
/// Initial hard ceiling for concurrently executing native browser contexts.
pub const MAX_EXECUTING_CONTEXTS: usize = 4;

#[derive(Debug)]
struct QueuedContext {
    identity: ContextIdentity,
    capabilities: ContextCapabilities,
}

struct ActiveContext {
    record: ContextRecord,
    executing: bool,
}

enum RegistryRow {
    Queued(QueuedContext),
    Active(ActiveContext),
}

/// Coarse registry state for one bounded scheduling projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextRegistryEntryState {
    /// Identity and capabilities are reserved but no native work has started.
    Queued,
    /// Native lifecycle exists and has the attached status.
    Active(ContextStatus),
}

/// Privacy-preserving registry projection for one context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextRegistryEntry {
    identity: ContextIdentity,
    state: ContextRegistryEntryState,
    executing: bool,
}

impl ContextRegistryEntry {
    /// Immutable context/run/profile/kind identity.
    pub const fn identity(self) -> ContextIdentity {
        self.identity
    }

    /// Queued or active lifecycle projection.
    pub const fn state(self) -> ContextRegistryEntryState {
        self.state
    }

    /// Whether this row owns one of the four execution permits.
    pub const fn executing(self) -> bool {
        self.executing
    }
}

/// Aggregate counts used by scheduling, diagnostics, and shutdown gates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextRegistryStatus {
    total: u8,
    queued: u8,
    active: u8,
    executing: u8,
    owned_native_reservations: u8,
    borrowed_tab_leases: u8,
    terminal_pending_reap: u8,
    sealed: bool,
}

impl ContextRegistryStatus {
    /// Total queued plus active rows.
    pub const fn total(self) -> u8 {
        self.total
    }

    /// Logical rows that have not started native work.
    pub const fn queued(self) -> u8 {
        self.queued
    }

    /// Rows with a lifecycle aggregate.
    pub const fn active(self) -> u8 {
        self.active
    }

    /// Rows holding an execution permit.
    pub const fn executing(self) -> u8 {
        self.executing
    }

    /// Nonterminal owned/handoff native-resource reservations.
    pub const fn owned_native_reservations(self) -> u8 {
        self.owned_native_reservations
    }

    /// Nonterminal leases over existing normal tabs.
    pub const fn borrowed_tab_leases(self) -> u8 {
        self.borrowed_tab_leases
    }

    /// Terminal rows awaiting exact resource-disposition observation.
    pub const fn terminal_pending_reap(self) -> u8 {
        self.terminal_pending_reap
    }

    /// Whether new reservations and construction are permanently sealed.
    pub const fn sealed(self) -> bool {
        self.sealed
    }
}

/// Resource disposition proven before a terminal row leaves the registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextResourceDisposition {
    /// Owned or handoff native resource was destroyed.
    Destroyed,
    /// Owned native resource was atomically transferred to the Browse pool.
    TransferredToBrowse,
    /// Borrowed tab remained in Browse and only its automation lease ended.
    ExistingBrowseRetained,
}

/// Exact terminal identity and native-resource disposition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetiredContext {
    identity: ContextIdentity,
    terminal: ContextTerminal,
    resource: ContextResourceDisposition,
}

impl RetiredContext {
    /// Immutable context identity that reached a terminal disposition.
    pub const fn identity(self) -> ContextIdentity {
        self.identity
    }

    /// Exact terminal lifecycle result.
    pub const fn terminal(self) -> ContextTerminal {
        self.terminal
    }

    /// Proven physical-resource disposition.
    pub const fn resource(self) -> ContextResourceDisposition {
        self.resource
    }
}

/// Typed bounded-registry refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContextRegistryError {
    /// New contexts are forbidden after the shutdown seal.
    #[error("context registry is sealed")]
    Sealed,
    /// The logical context ceiling is full.
    #[error("live context ceiling exceeded")]
    LiveLimit,
    /// All execution permits are in use.
    #[error("executing context ceiling exceeded")]
    ExecutionLimit,
    /// A context with the same durable identity already exists.
    #[error("duplicate context identity")]
    Duplicate,
    /// No row exists for the exact identity.
    #[error("context identity not found")]
    NotFound,
    /// The operation requires a queued row.
    #[error("context is not queued")]
    NotQueued,
    /// The operation requires an active row.
    #[error("context is not active")]
    NotActive,
    /// The operation requires a terminal active row.
    #[error("context is not terminal")]
    NotTerminal,
    /// The complete capability set belongs to another context kind.
    #[error("context capability kind mismatch")]
    CapabilityKind,
    /// The context lifecycle refused the exact transition.
    #[error(transparent)]
    Transition(#[from] ContextTransitionError),
    /// Internal accounting or terminal disposition became contradictory.
    #[error("context registry accounting invariant failed")]
    Invariant,
}

/// Bounded process-local authority for every agent-browser context row.
#[derive(Default)]
pub struct ContextRegistry {
    rows: BTreeMap<ContextId, RegistryRow>,
    sealed: bool,
}

impl ContextRegistry {
    /// Creates an empty registry with no allocation, worker, timer, or native cost.
    pub const fn new() -> Self {
        Self {
            rows: BTreeMap::new(),
            sealed: false,
        }
    }

    /// Reserves one logical row without starting native construction.
    pub fn reserve(
        &mut self,
        identity: ContextIdentity,
        capabilities: ContextCapabilities,
    ) -> Result<(), ContextRegistryError> {
        self.validate()?;
        if self.sealed {
            return Err(ContextRegistryError::Sealed);
        }
        if capabilities.kind() != identity.kind() {
            return Err(ContextRegistryError::CapabilityKind);
        }
        if self.rows.contains_key(&identity.id()) {
            return Err(ContextRegistryError::Duplicate);
        }
        if self.rows.len() >= MAX_LIVE_CONTEXTS {
            return Err(ContextRegistryError::LiveLimit);
        }
        self.rows.insert(
            identity.id(),
            RegistryRow::Queued(QueuedContext {
                identity,
                capabilities,
            }),
        );
        self.validate()
    }

    /// Cancels one never-started row and returns its immutable identity.
    pub fn cancel_queued(
        &mut self,
        context: ContextId,
    ) -> Result<ContextIdentity, ContextRegistryError> {
        self.validate()?;
        let row = self
            .rows
            .remove(&context)
            .ok_or(ContextRegistryError::NotFound)?;
        match row {
            RegistryRow::Queued(queued) => {
                self.validate()?;
                Ok(queued.identity)
            }
            active @ RegistryRow::Active(_) => {
                self.rows.insert(context, active);
                Err(ContextRegistryError::NotQueued)
            }
        }
    }

    /// Acquires one execution permit and starts exact native construction.
    pub fn begin_context(
        &mut self,
        context: ContextId,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextRegistryError> {
        self.validate()?;
        if self.sealed {
            return Err(ContextRegistryError::Sealed);
        }
        match self.rows.get(&context) {
            Some(RegistryRow::Queued(_)) => {}
            Some(RegistryRow::Active(_)) => return Err(ContextRegistryError::NotQueued),
            None => return Err(ContextRegistryError::NotFound),
        }
        if self.executing_count() >= MAX_EXECUTING_CONTEXTS {
            return Err(ContextRegistryError::ExecutionLimit);
        }
        let row = self
            .rows
            .remove(&context)
            .ok_or(ContextRegistryError::Invariant)?;
        let RegistryRow::Queued(queued) = row else {
            self.rows.insert(context, row);
            return Err(ContextRegistryError::NotQueued);
        };
        let (record, join) =
            match ContextRecord::begin(queued.identity, queued.capabilities, operation) {
                Ok(result) => result,
                Err(error) => {
                    self.rows.insert(context, RegistryRow::Queued(queued));
                    return Err(error.into());
                }
            };
        self.rows.insert(
            context,
            RegistryRow::Active(ActiveContext {
                record,
                executing: true,
            }),
        );
        self.validate()?;
        Ok(join)
    }

    /// Returns one bounded registry projection.
    pub fn entry(&self, context: ContextId) -> Option<ContextRegistryEntry> {
        match self.rows.get(&context)? {
            RegistryRow::Queued(queued) => Some(ContextRegistryEntry {
                identity: queued.identity,
                state: ContextRegistryEntryState::Queued,
                executing: false,
            }),
            RegistryRow::Active(active) => Some(ContextRegistryEntry {
                identity: active.record.identity(),
                state: ContextRegistryEntryState::Active(active.record.status()),
                executing: active.executing,
            }),
        }
    }

    /// Returns every row in stable `ContextId` order under the hard ceiling.
    pub fn entries(&self) -> Vec<ContextRegistryEntry> {
        self.rows
            .keys()
            .filter_map(|context| self.entry(*context))
            .collect()
    }

    /// Returns current aggregate resource and shutdown counts.
    pub fn status(&self) -> ContextRegistryStatus {
        let mut queued = 0_u8;
        let mut active = 0_u8;
        let mut executing = 0_u8;
        let mut owned_native_reservations = 0_u8;
        let mut borrowed_tab_leases = 0_u8;
        let mut terminal_pending_reap = 0_u8;
        for row in self.rows.values() {
            match row {
                RegistryRow::Queued(_) => queued = queued.saturating_add(1),
                RegistryRow::Active(active_row) => {
                    active = active.saturating_add(1);
                    executing = executing.saturating_add(u8::from(active_row.executing));
                    let terminal = matches!(
                        active_row.record.status().lifecycle(),
                        ContextLifecycle::Terminal(_)
                    );
                    terminal_pending_reap =
                        terminal_pending_reap.saturating_add(u8::from(terminal));
                    if !terminal {
                        match active_row.record.identity().kind() {
                            ContextKind::Owned | ContextKind::HumanSignInHandoff => {
                                owned_native_reservations =
                                    owned_native_reservations.saturating_add(1);
                            }
                            ContextKind::BorrowedTab => {
                                borrowed_tab_leases = borrowed_tab_leases.saturating_add(1);
                            }
                        }
                    }
                }
            }
        }
        ContextRegistryStatus {
            total: u8::try_from(self.rows.len()).unwrap_or(u8::MAX),
            queued,
            active,
            executing,
            owned_native_reservations,
            borrowed_tab_leases,
            terminal_pending_reap,
            sealed: self.sealed,
        }
    }

    /// Returns active/queued ids owned by one exact run in stable order.
    pub fn contexts_for_run(&self, owner: ContextRunId) -> Vec<ContextId> {
        self.rows
            .iter()
            .filter_map(|(id, row)| (row_identity(row).owner() == owner).then_some(*id))
            .collect()
    }

    /// Returns active/queued ids bound to one exact profile in stable order.
    pub fn contexts_for_profile(&self, profile: ProfileId) -> Vec<ContextId> {
        self.rows
            .iter()
            .filter_map(|(id, row)| (row_identity(row).profile() == profile).then_some(*id))
            .collect()
    }

    /// Permanently seals admissions and removes all never-started rows.
    pub fn seal_for_shutdown(&mut self) -> Result<Vec<ContextIdentity>, ContextRegistryError> {
        self.validate()?;
        self.sealed = true;
        let queued = self
            .rows
            .iter()
            .filter_map(|(id, row)| matches!(row, RegistryRow::Queued(_)).then_some(*id))
            .collect::<Vec<_>>();
        let mut removed = Vec::with_capacity(queued.len());
        for id in queued {
            let Some(RegistryRow::Queued(row)) = self.rows.remove(&id) else {
                return Err(ContextRegistryError::Invariant);
            };
            removed.push(row.identity);
        }
        self.validate()?;
        Ok(removed)
    }

    /// Returns exact active ids that must close or release during shutdown.
    pub fn shutdown_targets(&self) -> Vec<ContextId> {
        self.rows
            .iter()
            .filter_map(|(id, row)| matches!(row, RegistryRow::Active(_)).then_some(*id))
            .collect()
    }

    /// True only after the seal and exact removal of every context row.
    pub fn is_quiescent(&self) -> bool {
        self.sealed && self.rows.is_empty()
    }

    /// Current complete join for one active context.
    pub fn join(&self, context: ContextId) -> Result<ContextJoin, ContextRegistryError> {
        Ok(self.active(context)?.record.join())
    }

    /// Settles exact initial native construction.
    pub fn settle_construction(
        &mut self,
        context: ContextId,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextRegistryError> {
        let active = self.active_mut(context)?;
        active.record.settle_construction(join, settlement)?;
        if settlement == ContextSettlement::Refused
            && !active.record.status().native_view_resident()
        {
            active.executing = false;
        }
        self.validate()
    }

    /// Marks one semantic observation current for an exact active context.
    pub fn acknowledge_observation(
        &mut self,
        context: ContextId,
        join: ContextJoin,
    ) -> Result<(), ContextRegistryError> {
        self.active_mut(context)?
            .record
            .acknowledge_observation(join)?;
        self.validate()
    }

    /// Starts one exact navigation.
    pub fn begin_navigation(
        &mut self,
        context: ContextId,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextRegistryError> {
        let result = self
            .active_mut(context)?
            .record
            .begin_navigation(operation)?;
        self.validate()?;
        Ok(result)
    }

    /// Settles one exact navigation.
    pub fn settle_navigation(
        &mut self,
        context: ContextId,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextRegistryError> {
        self.active_mut(context)?
            .record
            .settle_navigation(join, settlement)?;
        self.validate()
    }

    /// Records a page-initiated or human navigation replacement.
    pub fn observe_navigation_replacement(
        &mut self,
        context: ContextId,
        prior: ContextJoin,
    ) -> Result<ContextJoin, ContextRegistryError> {
        let result = self
            .active_mut(context)?
            .record
            .observe_navigation_replacement(prior)?;
        self.validate()?;
        Ok(result)
    }

    /// Presents a context for inspection without transferring input.
    pub fn show_for_inspection(&mut self, context: ContextId) -> Result<(), ContextRegistryError> {
        self.active_mut(context)?.record.show_for_inspection()?;
        self.validate()
    }

    /// Hides one agent-controlled context.
    pub fn hide(&mut self, context: ContextId) -> Result<(), ContextRegistryError> {
        self.active_mut(context)?.record.hide()?;
        self.validate()
    }

    /// Transfers exclusive input to a person, preempting pending navigation.
    pub fn begin_human_control(
        &mut self,
        context: ContextId,
    ) -> Result<ContextJoin, ContextRegistryError> {
        let result = self.active_mut(context)?.record.begin_human_control()?;
        self.validate()?;
        Ok(result)
    }

    /// Returns input to the agent under a fresh-observation requirement.
    pub fn end_human_control(
        &mut self,
        context: ContextId,
    ) -> Result<ContextJoin, ContextRegistryError> {
        let result = self.active_mut(context)?.record.end_human_control()?;
        self.validate()?;
        Ok(result)
    }

    /// Starts exact hidden-context suspension.
    pub fn begin_suspend(
        &mut self,
        context: ContextId,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextRegistryError> {
        let result = self.active_mut(context)?.record.begin_suspend(operation)?;
        self.validate()?;
        Ok(result)
    }

    /// Settles suspension and releases the execution permit only on success.
    pub fn settle_suspend(
        &mut self,
        context: ContextId,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextRegistryError> {
        let active = self.active_mut(context)?;
        active.record.settle_suspend(join, settlement)?;
        if settlement == ContextSettlement::Applied {
            active.executing = false;
        }
        self.validate()
    }

    /// Acquires an execution permit and starts resume.
    pub fn begin_resume(
        &mut self,
        context: ContextId,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextRegistryError> {
        if self.active(context)?.record.status().lifecycle() != ContextLifecycle::Suspended {
            return Err(ContextTransitionError::InvalidLifecycle.into());
        }
        self.acquire_execution(context)?;
        match self.active_mut(context)?.record.begin_resume(operation) {
            Ok(join) => {
                self.validate()?;
                Ok(join)
            }
            Err(error) => {
                self.active_mut(context)?.executing = false;
                Err(error.into())
            }
        }
    }

    /// Settles resume and returns its permit if native resume refused.
    pub fn settle_resume(
        &mut self,
        context: ContextId,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextRegistryError> {
        let active = self.active_mut(context)?;
        active.record.settle_resume(join, settlement)?;
        if settlement == ContextSettlement::Refused {
            active.executing = false;
        }
        self.validate()
    }

    /// Records exact renderer loss and immediately releases its execution permit.
    pub fn renderer_lost(
        &mut self,
        context: ContextId,
        prior: ContextJoin,
    ) -> Result<ContextJoin, ContextRegistryError> {
        let active = self.active_mut(context)?;
        let join = active.record.renderer_lost(prior)?;
        active.executing = false;
        self.validate()?;
        Ok(join)
    }

    /// Acquires an execution permit and starts deferred renderer recovery.
    pub fn begin_recovery(
        &mut self,
        context: ContextId,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextRegistryError> {
        if self.active(context)?.record.status().lifecycle() != ContextLifecycle::RendererLost {
            return Err(ContextTransitionError::InvalidLifecycle.into());
        }
        self.acquire_execution(context)?;
        match self.active_mut(context)?.record.begin_recovery(operation) {
            Ok(join) => {
                self.validate()?;
                Ok(join)
            }
            Err(error) => {
                self.active_mut(context)?.executing = false;
                Err(error.into())
            }
        }
    }

    /// Settles recovery and returns its permit when recovery refused.
    pub fn settle_recovery(
        &mut self,
        context: ContextId,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextRegistryError> {
        let active = self.active_mut(context)?;
        active.record.settle_recovery(join, settlement)?;
        if settlement == ContextSettlement::Refused {
            active.executing = false;
        }
        self.validate()
    }

    /// Applies sticky run cancellation and invalidates old joins.
    pub fn cancel_run(
        &mut self,
        context: ContextId,
        prior: ContextJoin,
    ) -> Result<ContextJoin, ContextRegistryError> {
        let active = self.active_mut(context)?;
        let join = active.record.cancel_run(prior)?;
        if active.record.status().lifecycle() == ContextLifecycle::Faulted
            && !active.record.status().native_view_resident()
        {
            active.executing = false;
        }
        self.validate()?;
        Ok(join)
    }

    /// Starts owned-context native teardown.
    pub fn begin_close(
        &mut self,
        context: ContextId,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextRegistryError> {
        let result = self.active_mut(context)?.record.begin_close(operation)?;
        self.validate()?;
        Ok(result)
    }

    /// Settles owned-context native teardown.
    pub fn settle_close(
        &mut self,
        context: ContextId,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextRegistryError> {
        let active = self.active_mut(context)?;
        active.record.settle_close(join, settlement)?;
        if settlement == ContextSettlement::Applied {
            active.executing = false;
        }
        self.validate()
    }

    /// Starts explicit human-authorized adoption into Browse.
    pub fn begin_adoption(
        &mut self,
        context: ContextId,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextRegistryError> {
        let result = self.active_mut(context)?.record.begin_adoption(operation)?;
        self.validate()?;
        Ok(result)
    }

    /// Settles adoption; successful transfer releases the execution permit.
    pub fn settle_adoption(
        &mut self,
        context: ContextId,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextRegistryError> {
        let active = self.active_mut(context)?;
        active.record.settle_adoption(join, settlement)?;
        if settlement == ContextSettlement::Applied {
            active.executing = false;
        }
        self.validate()
    }

    /// Starts release of a borrowed tab or sign-in handoff.
    pub fn begin_release(
        &mut self,
        context: ContextId,
        operation: ContextOperationId,
    ) -> Result<ContextOperationJoin, ContextRegistryError> {
        let result = self.active_mut(context)?.record.begin_release(operation)?;
        self.validate()?;
        Ok(result)
    }

    /// Settles release and returns its execution permit on success.
    pub fn settle_release(
        &mut self,
        context: ContextId,
        join: ContextOperationJoin,
        settlement: ContextSettlement,
    ) -> Result<(), ContextRegistryError> {
        let active = self.active_mut(context)?;
        active.record.settle_release(join, settlement)?;
        if settlement == ContextSettlement::Applied {
            active.executing = false;
        }
        self.validate()
    }

    /// Observes and removes one exact terminal resource disposition.
    pub fn reap_terminal(
        &mut self,
        context: ContextId,
    ) -> Result<RetiredContext, ContextRegistryError> {
        self.validate()?;
        let row = self
            .rows
            .remove(&context)
            .ok_or(ContextRegistryError::NotFound)?;
        let RegistryRow::Active(active) = row else {
            self.rows.insert(context, row);
            return Err(ContextRegistryError::NotActive);
        };
        let ContextLifecycle::Terminal(terminal) = active.record.status().lifecycle() else {
            self.rows.insert(context, RegistryRow::Active(active));
            return Err(ContextRegistryError::NotTerminal);
        };
        if active.executing || active.record.status().native_view_resident() {
            self.rows.insert(context, RegistryRow::Active(active));
            return Err(ContextRegistryError::Invariant);
        }
        let identity = active.record.identity();
        let resource = match (identity.kind(), terminal) {
            (ContextKind::Owned, ContextTerminal::Closed)
            | (ContextKind::HumanSignInHandoff, ContextTerminal::Released) => {
                ContextResourceDisposition::Destroyed
            }
            (ContextKind::Owned, ContextTerminal::Adopted) => {
                ContextResourceDisposition::TransferredToBrowse
            }
            (ContextKind::BorrowedTab, ContextTerminal::Released) => {
                ContextResourceDisposition::ExistingBrowseRetained
            }
            _ => {
                self.rows.insert(context, RegistryRow::Active(active));
                return Err(ContextRegistryError::Invariant);
            }
        };
        self.validate()?;
        Ok(RetiredContext {
            identity,
            terminal,
            resource,
        })
    }

    fn acquire_execution(&mut self, context: ContextId) -> Result<(), ContextRegistryError> {
        if self.active(context)?.executing {
            return Err(ContextRegistryError::Invariant);
        }
        if self.executing_count() >= MAX_EXECUTING_CONTEXTS {
            return Err(ContextRegistryError::ExecutionLimit);
        }
        let active = self.active_mut(context)?;
        active.executing = true;
        Ok(())
    }

    fn active(&self, context: ContextId) -> Result<&ActiveContext, ContextRegistryError> {
        match self.rows.get(&context) {
            Some(RegistryRow::Active(active)) => Ok(active),
            Some(RegistryRow::Queued(_)) => Err(ContextRegistryError::NotActive),
            None => Err(ContextRegistryError::NotFound),
        }
    }

    fn active_mut(
        &mut self,
        context: ContextId,
    ) -> Result<&mut ActiveContext, ContextRegistryError> {
        match self.rows.get_mut(&context) {
            Some(RegistryRow::Active(active)) => Ok(active),
            Some(RegistryRow::Queued(_)) => Err(ContextRegistryError::NotActive),
            None => Err(ContextRegistryError::NotFound),
        }
    }

    fn executing_count(&self) -> usize {
        self.rows
            .values()
            .filter(|row| matches!(row, RegistryRow::Active(active) if active.executing))
            .count()
    }

    fn validate(&self) -> Result<(), ContextRegistryError> {
        if self.rows.len() > MAX_LIVE_CONTEXTS || self.executing_count() > MAX_EXECUTING_CONTEXTS {
            return Err(ContextRegistryError::Invariant);
        }
        for (id, row) in &self.rows {
            let identity = row_identity(row);
            if identity.id() != *id {
                return Err(ContextRegistryError::Invariant);
            }
            match row {
                RegistryRow::Queued(queued) => {
                    if self.sealed || queued.capabilities.kind() != identity.kind() {
                        return Err(ContextRegistryError::Invariant);
                    }
                }
                RegistryRow::Active(active) => {
                    let status = active.record.status();
                    if active.record.capabilities().kind() != identity.kind()
                        || (active.executing
                            && matches!(
                                status.lifecycle(),
                                ContextLifecycle::Suspended
                                    | ContextLifecycle::RendererLost
                                    | ContextLifecycle::Terminal(_)
                            ))
                        || (matches!(status.lifecycle(), ContextLifecycle::Terminal(_))
                            && status.native_view_resident())
                    {
                        return Err(ContextRegistryError::Invariant);
                    }
                }
            }
        }
        Ok(())
    }
}

fn row_identity(row: &RegistryRow) -> ContextIdentity {
    match row {
        RegistryRow::Queued(queued) => queued.identity,
        RegistryRow::Active(active) => active.record.identity(),
    }
}

impl std::fmt::Debug for ContextRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextRegistry")
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextCapability, ContextControl, ContextFreshness, ContextVisibility};

    fn context(value: u128) -> ContextId {
        ContextId::from_raw(value)
    }

    fn run(value: u128) -> ContextRunId {
        ContextRunId::from_raw(value)
    }

    fn operation(value: u64) -> ContextOperationId {
        ContextOperationId::new(value).expect("operation")
    }

    fn identity(value: u128, kind: ContextKind) -> ContextIdentity {
        ContextIdentity::new(context(value), run(10), ProfileId::from(20), kind)
    }

    fn capabilities(kind: ContextKind) -> ContextCapabilities {
        let values: &[ContextCapability] = match kind {
            ContextKind::Owned => &[
                ContextCapability::Navigate,
                ContextCapability::Observe,
                ContextCapability::Present,
                ContextCapability::Suspend,
                ContextCapability::Recover,
                ContextCapability::HumanControl,
                ContextCapability::Adopt,
            ],
            ContextKind::BorrowedTab => &[
                ContextCapability::Observe,
                ContextCapability::Present,
                ContextCapability::Suspend,
                ContextCapability::Recover,
                ContextCapability::HumanControl,
                ContextCapability::Release,
            ],
            ContextKind::HumanSignInHandoff => &[
                ContextCapability::Navigate,
                ContextCapability::Present,
                ContextCapability::Recover,
                ContextCapability::HumanControl,
                ContextCapability::ExportCookies,
                ContextCapability::Release,
            ],
        };
        ContextCapabilities::try_new(kind, values).expect("capabilities")
    }

    fn reserve(registry: &mut ContextRegistry, value: u128, kind: ContextKind) {
        registry
            .reserve(identity(value, kind), capabilities(kind))
            .expect("reserve");
    }

    fn activate(
        registry: &mut ContextRegistry,
        value: u128,
        operation_value: u64,
    ) -> ContextOperationJoin {
        let join = registry
            .begin_context(context(value), operation(operation_value))
            .expect("begin");
        registry
            .settle_construction(context(value), join, ContextSettlement::Applied)
            .expect("construct");
        join
    }

    #[test]
    fn logical_and_execution_limits_refuse_without_eviction() {
        let mut registry = ContextRegistry::new();
        for value in 1..=MAX_LIVE_CONTEXTS as u128 {
            reserve(&mut registry, value, ContextKind::Owned);
        }
        assert_eq!(
            registry.reserve(
                identity(99, ContextKind::Owned),
                capabilities(ContextKind::Owned)
            ),
            Err(ContextRegistryError::LiveLimit)
        );
        for value in 1..=MAX_EXECUTING_CONTEXTS as u128 {
            activate(&mut registry, value, value as u64);
        }
        assert_eq!(
            registry.begin_context(context(5), operation(5)),
            Err(ContextRegistryError::ExecutionLimit)
        );
        assert_eq!(registry.status().total(), MAX_LIVE_CONTEXTS as u8);
        assert_eq!(registry.status().executing(), MAX_EXECUTING_CONTEXTS as u8);
    }

    #[test]
    fn successful_suspend_releases_and_resume_reacquires_exact_permit() {
        let mut registry = ContextRegistry::new();
        for value in 1..=5 {
            reserve(&mut registry, value, ContextKind::Owned);
        }
        for value in 1..=4 {
            activate(&mut registry, value, value as u64);
        }
        let suspend = registry
            .begin_suspend(context(1), operation(10))
            .expect("suspend");
        registry
            .settle_suspend(context(1), suspend, ContextSettlement::Applied)
            .expect("suspended");
        assert_eq!(registry.status().executing(), 3);
        activate(&mut registry, 5, 5);
        assert_eq!(
            registry.begin_resume(context(1), operation(11)),
            Err(ContextRegistryError::ExecutionLimit)
        );

        let close = registry
            .begin_close(context(2), operation(12))
            .expect("close");
        registry
            .settle_close(context(2), close, ContextSettlement::Applied)
            .expect("closed");
        let retired = registry.reap_terminal(context(2)).expect("reap");
        assert_eq!(retired.resource(), ContextResourceDisposition::Destroyed);
        let resume = registry
            .begin_resume(context(1), operation(13))
            .expect("resume");
        registry
            .settle_resume(context(1), resume, ContextSettlement::Refused)
            .expect("resume refused");
        assert_eq!(registry.status().executing(), 3);
    }

    #[test]
    fn renderer_loss_queues_recovery_under_the_same_execution_ceiling() {
        let mut registry = ContextRegistry::new();
        for value in 1..=5 {
            reserve(&mut registry, value, ContextKind::Owned);
        }
        for value in 1..=4 {
            activate(&mut registry, value, value as u64);
        }
        let old = registry.join(context(1)).expect("join");
        let lost = registry.renderer_lost(context(1), old).expect("lost");
        assert_eq!(registry.status().executing(), 3);
        let ContextRegistryEntryState::Active(status) =
            registry.entry(context(1)).expect("entry").state()
        else {
            panic!("active context");
        };
        assert_eq!(status.lifecycle(), ContextLifecycle::RendererLost);
        assert_eq!(status.visibility(), ContextVisibility::Hidden);
        assert_eq!(status.control(), ContextControl::Agent);
        assert_eq!(status.freshness(), ContextFreshness::ObservationRequired);
        assert!(!status.native_view_resident());
        assert!(!status.run_cancelled());
        assert_eq!(status.pending_operation(), None);
        activate(&mut registry, 5, 5);
        assert_eq!(
            registry.begin_recovery(context(1), operation(10)),
            Err(ContextRegistryError::ExecutionLimit)
        );
        assert_eq!(registry.join(context(1)).expect("still lost"), lost);
    }

    #[test]
    fn terminal_dispositions_are_kind_exact_and_release_all_counts() {
        let mut registry = ContextRegistry::new();
        reserve(&mut registry, 1, ContextKind::Owned);
        activate(&mut registry, 1, 1);
        registry
            .begin_human_control(context(1))
            .expect("human control");
        let adoption = registry
            .begin_adoption(context(1), operation(2))
            .expect("adopt");
        registry
            .settle_adoption(context(1), adoption, ContextSettlement::Applied)
            .expect("adopted");
        let adopted = registry.reap_terminal(context(1)).expect("reap");
        assert_eq!(
            adopted.resource(),
            ContextResourceDisposition::TransferredToBrowse
        );

        reserve(&mut registry, 2, ContextKind::BorrowedTab);
        activate(&mut registry, 2, 3);
        assert_eq!(registry.status().borrowed_tab_leases(), 1);
        let release = registry
            .begin_release(context(2), operation(4))
            .expect("release");
        registry
            .settle_release(context(2), release, ContextSettlement::Applied)
            .expect("released");
        let released = registry.reap_terminal(context(2)).expect("reap");
        assert_eq!(
            released.resource(),
            ContextResourceDisposition::ExistingBrowseRetained
        );
        assert_eq!(registry.status().total(), 0);
    }

    #[test]
    fn shutdown_seal_drops_only_queued_rows_and_requires_terminal_reap() {
        let mut registry = ContextRegistry::new();
        reserve(&mut registry, 1, ContextKind::Owned);
        reserve(&mut registry, 2, ContextKind::BorrowedTab);
        activate(&mut registry, 1, 1);
        let dropped = registry.seal_for_shutdown().expect("seal");
        assert_eq!(dropped, vec![identity(2, ContextKind::BorrowedTab)]);
        assert_eq!(registry.shutdown_targets(), vec![context(1)]);
        assert!(!registry.is_quiescent());
        assert_eq!(
            registry.reserve(
                identity(3, ContextKind::Owned),
                capabilities(ContextKind::Owned)
            ),
            Err(ContextRegistryError::Sealed)
        );
        let current = registry.join(context(1)).expect("join");
        registry.cancel_run(context(1), current).expect("cancel");
        let close = registry
            .begin_close(context(1), operation(2))
            .expect("close");
        registry
            .settle_close(context(1), close, ContextSettlement::Applied)
            .expect("closed");
        assert!(!registry.is_quiescent());
        registry.reap_terminal(context(1)).expect("reap");
        assert!(registry.is_quiescent());
    }

    #[test]
    fn run_and_profile_indexes_are_bounded_projections_not_authority() {
        let mut registry = ContextRegistry::new();
        reserve(&mut registry, 1, ContextKind::Owned);
        reserve(&mut registry, 2, ContextKind::BorrowedTab);
        assert_eq!(
            registry.contexts_for_run(run(10)),
            vec![context(1), context(2)]
        );
        assert_eq!(
            registry.contexts_for_profile(ProfileId::from(20)),
            vec![context(1), context(2)]
        );
        assert!(format!("{registry:?}").contains("total: 2"));
        assert!(!format!("{registry:?}").contains("000000"));
    }
}
