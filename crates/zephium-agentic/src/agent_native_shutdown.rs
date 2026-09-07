//! Bounded functional-core coordination for terminal native browser drain.
//!
//! The application actor first seals and drains every logical owner represented
//! by [`AgentNativeShutdownResources`], or obtains the one-shot admission from
//! its original sealed [`crate::WorkBrowserResources`]. Only then may it linearize
//! the native port's mutation seal, distinguish its exact barrier settlement
//! from ordinary audits, and require a validated all-zero resource snapshot.
//! It owns no port, clock, timer, task, worker, channel, browser object, or I/O.

use std::fmt;

use thiserror::Error;

use crate::{
    ContextCookieTransferRegistry, ContextDispatch, ContextNativeResourceSnapshot,
    ContextProfileLeaseRegistry, ContextRegistry, ContextResourceAuditId,
    ContextResourceAuditSettlement, ContextShutdownAuditSettlement, ContextShutdownDispatch,
    SemanticActionExecutionCoordinator, SemanticActionSettlementCoordinator,
    SemanticScreenshotCoordinator,
};

/// Maximum native resource-audit attempts in one process shutdown.
///
/// The initial atomic seal audit counts toward this ceiling. A shell may stop
/// earlier at its absolute process deadline, but it cannot use this coordinator
/// to create an unbounded audit loop.
pub const MAX_AGENT_NATIVE_SHUTDOWN_AUDITS: u8 = 8;

/// Maximum byte width of the content-free terminal native-drain proof.
pub const MAX_AGENT_NATIVE_SHUTDOWN_PROOF_BYTES: usize = 64;

/// Exact single-owner resources that must be sealed and empty before the
/// native browser port can be terminally sealed.
///
/// This cohort proves only browser-context, profile-lease, cookie-transfer,
/// action-execution, action-settlement, and screenshot drain. Run policy,
/// provider work, durable audit delivery, and engine teardown remain separate
/// owners and are deliberately not represented here.
#[must_use]
pub struct AgentNativeShutdownResources {
    contexts: ContextRegistry,
    profile_leases: ContextProfileLeaseRegistry,
    cookie_transfers: ContextCookieTransferRegistry,
    action_executions: SemanticActionExecutionCoordinator,
    action_settlements: SemanticActionSettlementCoordinator,
    screenshots: SemanticScreenshotCoordinator,
}

impl AgentNativeShutdownResources {
    /// Collects the exact process-local browser execution owners at shutdown.
    pub fn new(
        contexts: ContextRegistry,
        profile_leases: ContextProfileLeaseRegistry,
        cookie_transfers: ContextCookieTransferRegistry,
        action_executions: SemanticActionExecutionCoordinator,
        action_settlements: SemanticActionSettlementCoordinator,
        screenshots: SemanticScreenshotCoordinator,
    ) -> Self {
        Self {
            contexts,
            profile_leases,
            cookie_transfers,
            action_executions,
            action_settlements,
            screenshots,
        }
    }

    /// Mutable access for exact logical context cancellation and terminal reap.
    pub fn contexts_mut(&mut self) -> &mut ContextRegistry {
        &mut self.contexts
    }

    /// Mutable access for receipt-bound terminal profile-lease release.
    pub fn profile_leases_mut(&mut self) -> &mut ContextProfileLeaseRegistry {
        &mut self.profile_leases
    }

    /// Mutable access for exact pending cookie-transfer settlement.
    pub fn cookie_transfers_mut(&mut self) -> &mut ContextCookieTransferRegistry {
        &mut self.cookie_transfers
    }

    /// Mutable access for exact pending native-action terminalization.
    pub fn action_executions_mut(&mut self) -> &mut SemanticActionExecutionCoordinator {
        &mut self.action_executions
    }

    /// Mutable access for exact pending post-action settlement terminalization.
    pub fn action_settlements_mut(&mut self) -> &mut SemanticActionSettlementCoordinator {
        &mut self.action_settlements
    }

    /// Mutable access for exact pending native screenshot terminalization.
    pub fn screenshots_mut(&mut self) -> &mut SemanticScreenshotCoordinator {
        &mut self.screenshots
    }

    fn readiness_error(&self) -> Option<AgentNativeShutdownAdmissionError> {
        if !self.contexts.is_quiescent() {
            return Some(AgentNativeShutdownAdmissionError::Contexts);
        }
        if !self.profile_leases.is_quiescent() {
            return Some(AgentNativeShutdownAdmissionError::ProfileLeases);
        }
        if !self.cookie_transfers.is_quiescent() {
            return Some(AgentNativeShutdownAdmissionError::CookieTransfers);
        }
        let action_executions = self.action_executions.status();
        if !action_executions.sealed() || action_executions.pending() != 0 {
            return Some(AgentNativeShutdownAdmissionError::ActionExecutions);
        }
        let action_settlements = self.action_settlements.status();
        if !action_settlements.sealed() || action_settlements.pending() != 0 {
            return Some(AgentNativeShutdownAdmissionError::ActionSettlements);
        }
        if !self.screenshots.is_quiescent() {
            return Some(AgentNativeShutdownAdmissionError::Screenshots);
        }
        None
    }
}

impl fmt::Debug for AgentNativeShutdownResources {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNativeShutdownResources")
            .field("contexts", &self.contexts.status())
            .field("profile_leases", &self.profile_leases.status())
            .field("cookie_transfers", &self.cookie_transfers.status())
            .field("action_executions", &self.action_executions.status())
            .field("action_settlements", &self.action_settlements.status())
            .field("screenshots", &self.screenshots.status())
            .finish()
    }
}

/// Closed reason logical browser owners were not ready for native port seal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentNativeShutdownAdmissionError {
    /// Context admission was unsealed or a queued/active row remained.
    #[error("agent context registry is not shutdown-quiescent")]
    Contexts,
    /// Profile-lease admission was unsealed or an exact lease remained.
    #[error("agent profile leases are not shutdown-quiescent")]
    ProfileLeases,
    /// Cookie-transfer admission was unsealed or a transfer remained.
    #[error("agent cookie transfers are not shutdown-quiescent")]
    CookieTransfers,
    /// Native-action admission was unsealed or an execution remained.
    #[error("agent native actions are not shutdown-quiescent")]
    ActionExecutions,
    /// Post-action admission was unsealed or a settlement remained.
    #[error("agent action settlements are not shutdown-quiescent")]
    ActionSettlements,
    /// Screenshot admission was unsealed or a native capture remained.
    #[error("agent screenshots are not shutdown-quiescent")]
    Screenshots,
}

/// Lossless refusal to begin terminal native shutdown coordination.
#[must_use]
pub struct AgentNativeShutdownAdmissionRefusal {
    error: AgentNativeShutdownAdmissionError,
    resources: AgentNativeShutdownResources,
}

impl AgentNativeShutdownAdmissionRefusal {
    /// Exact closed readiness failure.
    pub const fn error(&self) -> AgentNativeShutdownAdmissionError {
        self.error
    }

    /// Returns every move-only owner for continued exact cleanup.
    pub fn into_resources(self) -> AgentNativeShutdownResources {
        self.resources
    }
}

impl fmt::Debug for AgentNativeShutdownAdmissionRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNativeShutdownAdmissionRefusal")
            .field("error", &self.error)
            .field("resources", &self.resources)
            .finish()
    }
}

/// Current closed phase of terminal native browser drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentNativeShutdownStage {
    /// Logical owners are consumed and the atomic native seal may begin.
    ReadyToSeal,
    /// The shell owes the synchronous result of `seal_for_shutdown`.
    SealDispatchPending,
    /// The distinct audit admitted with the native seal owes one event.
    ShutdownAuditPending,
    /// A prior attempt did not prove zero; one bounded ordinary audit may begin.
    ResourceAuditRequired,
    /// The shell owes the synchronous result of `audit_resources`.
    ResourceAuditDispatchPending,
    /// One ordinary post-seal resource audit owes its exact event.
    ResourceAuditPending,
    /// An exact all-zero native resource snapshot was observed after the seal.
    ZeroProven,
    /// The fixed audit-attempt ceiling was consumed without proving zero.
    Exhausted,
}

/// Content-free current native shutdown accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentNativeShutdownStatus {
    stage: AgentNativeShutdownStage,
    attempts: u8,
    attempts_remaining: u8,
}

impl AgentNativeShutdownStatus {
    /// Current exact coordination phase.
    pub const fn stage(self) -> AgentNativeShutdownStage {
        self.stage
    }

    /// Seal plus ordinary resource-audit attempts already started.
    pub const fn attempts(self) -> u8 {
        self.attempts
    }

    /// Remaining attempts under the fixed process shutdown ceiling.
    pub const fn attempts_remaining(self) -> u8 {
        self.attempts_remaining
    }
}

#[derive(Clone, Copy)]
enum NativeShutdownState {
    ReadyToSeal,
    SealDispatchPending(ContextResourceAuditId),
    ShutdownAuditPending(ContextResourceAuditId),
    ResourceAuditRequired,
    ResourceAuditDispatchPending(ContextResourceAuditId),
    ResourceAuditPending(ContextResourceAuditId),
    ZeroProven {
        audit: ContextResourceAuditId,
        snapshot: ContextNativeResourceSnapshot,
    },
    Exhausted,
}

/// Zero-idle owner of exact post-context native resource drain.
#[must_use]
pub struct AgentNativeShutdownCoordinator {
    state: NativeShutdownState,
    attempts: u8,
    last_audit: Option<ContextResourceAuditId>,
}

impl AgentNativeShutdownCoordinator {
    /// Consumes only a fully sealed and drained logical browser cohort.
    ///
    /// Refusal returns the complete cohort so the application actor can keep
    /// cancelling and settling exact retained obligations.
    pub fn try_new(
        resources: AgentNativeShutdownResources,
    ) -> Result<Self, Box<AgentNativeShutdownAdmissionRefusal>> {
        if let Some(error) = resources.readiness_error() {
            return Err(Box::new(AgentNativeShutdownAdmissionRefusal {
                error,
                resources,
            }));
        }
        Ok(Self::ready())
    }

    /// Only the original sealed retained registry can construct this operand.
    /// This reuses the same global seal, audit ceiling and all-zero predicate.
    pub(crate) fn from_retained_registry(
        _admission: crate::work_browser_resource::WorkBrowserNativeShutdownAdmission,
    ) -> Self {
        Self::ready()
    }

    fn ready() -> Self {
        Self {
            state: NativeShutdownState::ReadyToSeal,
            attempts: 0,
            last_audit: None,
        }
    }

    /// Reserves the sole atomic native-port seal attempt.
    ///
    /// After success, the shell must call `AgentBrowserPort::seal_for_shutdown`
    /// with this exact audit identity and account its synchronous result.
    pub fn begin_port_seal(
        &mut self,
        audit: ContextResourceAuditId,
    ) -> Result<(), AgentNativeShutdownError> {
        if !matches!(self.state, NativeShutdownState::ReadyToSeal) {
            return Err(AgentNativeShutdownError::Stage);
        }
        self.start_attempt(audit)?;
        self.state = NativeShutdownState::SealDispatchPending(audit);
        Ok(())
    }

    /// Accounts the exact synchronous result of the atomic native port seal.
    pub fn account_port_seal(
        &mut self,
        audit: ContextResourceAuditId,
        dispatch: ContextShutdownDispatch,
    ) -> Result<AgentNativeShutdownStatus, AgentNativeShutdownError> {
        require_audit(
            self.state,
            AgentNativeShutdownStage::SealDispatchPending,
            audit,
        )?;
        self.state = match dispatch {
            ContextShutdownDispatch::AuditScheduled => {
                NativeShutdownState::ShutdownAuditPending(audit)
            }
            ContextShutdownDispatch::SealedWithoutAudit(_) => self.retry_or_exhausted(),
        };
        Ok(self.status())
    }

    /// Begins one bounded read-only audit after the native mutation seal.
    pub fn begin_resource_audit(
        &mut self,
        audit: ContextResourceAuditId,
    ) -> Result<(), AgentNativeShutdownError> {
        if !matches!(self.state, NativeShutdownState::ResourceAuditRequired) {
            return Err(AgentNativeShutdownError::Stage);
        }
        self.start_attempt(audit)?;
        self.state = NativeShutdownState::ResourceAuditDispatchPending(audit);
        Ok(())
    }

    /// Accounts the exact synchronous result of one post-seal resource audit.
    pub fn account_resource_audit(
        &mut self,
        audit: ContextResourceAuditId,
        dispatch: ContextDispatch,
    ) -> Result<AgentNativeShutdownStatus, AgentNativeShutdownError> {
        require_audit(
            self.state,
            AgentNativeShutdownStage::ResourceAuditDispatchPending,
            audit,
        )?;
        self.state = match dispatch {
            ContextDispatch::Scheduled => NativeShutdownState::ResourceAuditPending(audit),
            ContextDispatch::Rejected(_) | ContextDispatch::Unsupported => {
                self.retry_or_exhausted()
            }
        };
        Ok(self.status())
    }

    /// Settles only the exact distinct audit emitted by the atomic port seal.
    pub fn settle_shutdown_audit(
        &mut self,
        settlement: ContextShutdownAuditSettlement,
    ) -> Result<AgentNativeShutdownStatus, AgentNativeShutdownError> {
        require_audit(
            self.state,
            AgentNativeShutdownStage::ShutdownAuditPending,
            settlement.audit(),
        )?;
        self.settle_snapshot(settlement.audit(), settlement.outcome());
        Ok(self.status())
    }

    /// Settles only the exact ordinary audit currently pending after the seal.
    pub fn settle_resource_audit(
        &mut self,
        settlement: ContextResourceAuditSettlement,
    ) -> Result<AgentNativeShutdownStatus, AgentNativeShutdownError> {
        require_audit(
            self.state,
            AgentNativeShutdownStage::ResourceAuditPending,
            settlement.audit(),
        )?;
        self.settle_snapshot(settlement.audit(), settlement.outcome());
        Ok(self.status())
    }

    /// Current content-free phase and fixed attempt accounting.
    pub fn status(&self) -> AgentNativeShutdownStatus {
        let stage = match self.state {
            NativeShutdownState::ReadyToSeal => AgentNativeShutdownStage::ReadyToSeal,
            NativeShutdownState::SealDispatchPending(_) => {
                AgentNativeShutdownStage::SealDispatchPending
            }
            NativeShutdownState::ShutdownAuditPending(_) => {
                AgentNativeShutdownStage::ShutdownAuditPending
            }
            NativeShutdownState::ResourceAuditRequired => {
                AgentNativeShutdownStage::ResourceAuditRequired
            }
            NativeShutdownState::ResourceAuditDispatchPending(_) => {
                AgentNativeShutdownStage::ResourceAuditDispatchPending
            }
            NativeShutdownState::ResourceAuditPending(_) => {
                AgentNativeShutdownStage::ResourceAuditPending
            }
            NativeShutdownState::ZeroProven { .. } => AgentNativeShutdownStage::ZeroProven,
            NativeShutdownState::Exhausted => AgentNativeShutdownStage::Exhausted,
        };
        AgentNativeShutdownStatus {
            stage,
            attempts: self.attempts,
            attempts_remaining: MAX_AGENT_NATIVE_SHUTDOWN_AUDITS.saturating_sub(self.attempts),
        }
    }

    /// Consumes the coordinator only after exact post-seal all-zero evidence.
    ///
    /// The proof is necessary but not sufficient for clean process shutdown:
    /// durable audit delivery, run-policy settlement, and engine teardown are
    /// independently owned and remain mandatory.
    pub fn finish(self) -> Result<AgentNativeShutdownProof, Box<AgentNativeShutdownFinishRefusal>> {
        let NativeShutdownState::ZeroProven { audit, snapshot } = self.state else {
            return Err(Box::new(AgentNativeShutdownFinishRefusal {
                coordinator: self,
            }));
        };
        Ok(AgentNativeShutdownProof {
            audit,
            attempts: self.attempts,
            snapshot,
        })
    }

    fn start_attempt(
        &mut self,
        audit: ContextResourceAuditId,
    ) -> Result<(), AgentNativeShutdownError> {
        if self
            .last_audit
            .is_some_and(|last_audit| audit <= last_audit)
        {
            return Err(AgentNativeShutdownError::AuditReplay);
        }
        let Some(attempts) = self.attempts.checked_add(1) else {
            return Err(AgentNativeShutdownError::Stage);
        };
        if attempts > MAX_AGENT_NATIVE_SHUTDOWN_AUDITS {
            return Err(AgentNativeShutdownError::Stage);
        }
        self.attempts = attempts;
        self.last_audit = Some(audit);
        Ok(())
    }

    fn settle_snapshot(
        &mut self,
        audit: ContextResourceAuditId,
        outcome: Result<ContextNativeResourceSnapshot, crate::ContextPortFailure>,
    ) {
        self.state = match outcome {
            Ok(snapshot) if native_snapshot_is_zero(snapshot) => {
                NativeShutdownState::ZeroProven { audit, snapshot }
            }
            Ok(_) | Err(_) => self.retry_or_exhausted(),
        };
    }

    fn retry_or_exhausted(&self) -> NativeShutdownState {
        if self.attempts >= MAX_AGENT_NATIVE_SHUTDOWN_AUDITS {
            NativeShutdownState::Exhausted
        } else {
            NativeShutdownState::ResourceAuditRequired
        }
    }
}

impl fmt::Debug for AgentNativeShutdownCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNativeShutdownCoordinator")
            .field("status", &self.status())
            .field("last_audit", &self.last_audit)
            .finish()
    }
}

/// Closed coordinator protocol refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentNativeShutdownError {
    /// The operation is not valid in the current exact shutdown phase.
    #[error("agent native shutdown operation is out of order")]
    Stage,
    /// A resource-audit identity was reused or regressed.
    #[error("agent native shutdown audit identity replayed")]
    AuditReplay,
    /// Dispatch or settlement did not name the exact pending audit.
    #[error("agent native shutdown audit identity mismatched")]
    AuditMismatch,
}

/// Lossless refusal to finish before native zero-resource proof exists.
#[must_use]
pub struct AgentNativeShutdownFinishRefusal {
    coordinator: AgentNativeShutdownCoordinator,
}

impl AgentNativeShutdownFinishRefusal {
    /// Returns the exact coordinator for continued bounded settlement.
    pub fn into_coordinator(self) -> AgentNativeShutdownCoordinator {
        self.coordinator
    }
}

impl fmt::Debug for AgentNativeShutdownFinishRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNativeShutdownFinishRefusal")
            .field("status", &self.coordinator.status())
            .finish()
    }
}

/// Constructor-closed evidence of an exact post-seal all-zero native audit.
#[must_use]
pub struct AgentNativeShutdownProof {
    audit: ContextResourceAuditId,
    attempts: u8,
    snapshot: ContextNativeResourceSnapshot,
}

impl AgentNativeShutdownProof {
    /// Exact terminal resource-audit identity.
    pub const fn audit(&self) -> ContextResourceAuditId {
        self.audit
    }

    /// Total bounded audit attempts required to prove zero.
    pub const fn attempts(&self) -> u8 {
        self.attempts
    }

    /// Validated terminal all-zero native resource snapshot.
    pub const fn snapshot(&self) -> ContextNativeResourceSnapshot {
        self.snapshot
    }
}

impl fmt::Debug for AgentNativeShutdownProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNativeShutdownProof")
            .field("audit", &self.audit)
            .field("attempts", &self.attempts)
            .field("snapshot", &self.snapshot)
            .finish()
    }
}

const _: () = assert!(
    std::mem::size_of::<AgentNativeShutdownProof>() <= MAX_AGENT_NATIVE_SHUTDOWN_PROOF_BYTES
);

fn require_audit(
    state: NativeShutdownState,
    expected_stage: AgentNativeShutdownStage,
    audit: ContextResourceAuditId,
) -> Result<(), AgentNativeShutdownError> {
    let (stage, expected_audit) = match state {
        NativeShutdownState::SealDispatchPending(expected) => {
            (AgentNativeShutdownStage::SealDispatchPending, expected)
        }
        NativeShutdownState::ShutdownAuditPending(expected) => {
            (AgentNativeShutdownStage::ShutdownAuditPending, expected)
        }
        NativeShutdownState::ResourceAuditDispatchPending(expected) => (
            AgentNativeShutdownStage::ResourceAuditDispatchPending,
            expected,
        ),
        NativeShutdownState::ResourceAuditPending(expected) => {
            (AgentNativeShutdownStage::ResourceAuditPending, expected)
        }
        _ => return Err(AgentNativeShutdownError::Stage),
    };
    if stage != expected_stage {
        return Err(AgentNativeShutdownError::Stage);
    }
    if audit != expected_audit {
        return Err(AgentNativeShutdownError::AuditMismatch);
    }
    Ok(())
}

fn native_snapshot_is_zero(snapshot: ContextNativeResourceSnapshot) -> bool {
    let counts = snapshot.counts();
    counts.known_bindings == 0
        && counts.resident_views == 0
        && counts.owned_reservations == 0
        && counts.borrowed_leases == 0
        && counts.visible_surfaces == 0
        && counts.suspended_views == 0
        && counts.pending_operations == 0
        && counts.pending_captures == 0
        && counts.queued_tasks == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextNativeResourceCounts, ContextPortFailure};

    #[derive(Clone, Copy)]
    enum OpenResource {
        None,
        Contexts,
        ProfileLeases,
        CookieTransfers,
        ActionExecutions,
        ActionSettlements,
        Screenshots,
    }

    fn resources(open: OpenResource) -> AgentNativeShutdownResources {
        let mut contexts = ContextRegistry::new();
        let mut profile_leases = ContextProfileLeaseRegistry::new();
        let mut cookie_transfers = ContextCookieTransferRegistry::new();
        let mut action_executions = SemanticActionExecutionCoordinator::new();
        let mut action_settlements = SemanticActionSettlementCoordinator::new();
        let mut screenshots = SemanticScreenshotCoordinator::new();
        if !matches!(open, OpenResource::Contexts) {
            contexts.seal_for_shutdown().expect("context seal");
        }
        if !matches!(open, OpenResource::ProfileLeases) {
            profile_leases
                .seal_for_shutdown()
                .expect("profile lease seal");
        }
        if !matches!(open, OpenResource::CookieTransfers) {
            cookie_transfers
                .seal_for_shutdown()
                .expect("cookie transfer seal");
        }
        if !matches!(open, OpenResource::ActionExecutions) {
            action_executions.seal();
        }
        if !matches!(open, OpenResource::ActionSettlements) {
            action_settlements.seal();
        }
        if !matches!(open, OpenResource::Screenshots) {
            screenshots.seal_for_shutdown();
        }
        AgentNativeShutdownResources::new(
            contexts,
            profile_leases,
            cookie_transfers,
            action_executions,
            action_settlements,
            screenshots,
        )
    }

    fn coordinator() -> AgentNativeShutdownCoordinator {
        AgentNativeShutdownCoordinator::try_new(resources(OpenResource::None))
            .expect("drained resources")
    }

    fn audit(value: u64) -> ContextResourceAuditId {
        ContextResourceAuditId::new(value).expect("audit")
    }

    fn snapshot(counts: ContextNativeResourceCounts) -> ContextNativeResourceSnapshot {
        ContextNativeResourceSnapshot::try_new(counts).expect("valid snapshot")
    }

    fn zero_snapshot() -> ContextNativeResourceSnapshot {
        snapshot(ContextNativeResourceCounts {
            known_bindings: 0,
            resident_views: 0,
            owned_reservations: 0,
            borrowed_leases: 0,
            visible_surfaces: 0,
            suspended_views: 0,
            pending_operations: 0,
            pending_captures: 0,
            queued_tasks: 0,
        })
    }

    #[test]
    fn admission_consumes_only_the_complete_sealed_drained_cohort() {
        for (open, expected) in [
            (
                OpenResource::Contexts,
                AgentNativeShutdownAdmissionError::Contexts,
            ),
            (
                OpenResource::ProfileLeases,
                AgentNativeShutdownAdmissionError::ProfileLeases,
            ),
            (
                OpenResource::CookieTransfers,
                AgentNativeShutdownAdmissionError::CookieTransfers,
            ),
            (
                OpenResource::ActionExecutions,
                AgentNativeShutdownAdmissionError::ActionExecutions,
            ),
            (
                OpenResource::ActionSettlements,
                AgentNativeShutdownAdmissionError::ActionSettlements,
            ),
            (
                OpenResource::Screenshots,
                AgentNativeShutdownAdmissionError::Screenshots,
            ),
        ] {
            let refusal = AgentNativeShutdownCoordinator::try_new(resources(open))
                .expect_err("open resource must be returned");
            assert_eq!(refusal.error(), expected);
            let _returned = refusal.into_resources();
        }
        assert_eq!(
            coordinator().status(),
            AgentNativeShutdownStatus {
                stage: AgentNativeShutdownStage::ReadyToSeal,
                attempts: 0,
                attempts_remaining: MAX_AGENT_NATIVE_SHUTDOWN_AUDITS,
            }
        );
    }

    #[test]
    fn exact_barrier_then_repeated_audit_proves_zero_once() {
        let mut coordinator = coordinator();
        coordinator.begin_port_seal(audit(10)).expect("begin seal");
        assert_eq!(
            coordinator
                .account_port_seal(audit(10), ContextShutdownDispatch::AuditScheduled)
                .expect("account seal")
                .stage(),
            AgentNativeShutdownStage::ShutdownAuditPending
        );
        assert_eq!(
            coordinator
                .settle_shutdown_audit(ContextShutdownAuditSettlement::new(
                    audit(10),
                    Ok(snapshot(ContextNativeResourceCounts {
                        known_bindings: 0,
                        resident_views: 0,
                        owned_reservations: 0,
                        borrowed_leases: 0,
                        visible_surfaces: 0,
                        suspended_views: 0,
                        pending_operations: 0,
                        pending_captures: 0,
                        queued_tasks: 1,
                    })),
                ))
                .expect("nonzero barrier audit")
                .stage(),
            AgentNativeShutdownStage::ResourceAuditRequired
        );

        coordinator
            .begin_resource_audit(audit(11))
            .expect("begin first retry");
        assert_eq!(
            coordinator
                .account_resource_audit(
                    audit(11),
                    ContextDispatch::Rejected(ContextPortFailure::ResourceExhausted),
                )
                .expect("account refusal")
                .stage(),
            AgentNativeShutdownStage::ResourceAuditRequired
        );
        coordinator
            .begin_resource_audit(audit(12))
            .expect("begin second retry");
        coordinator
            .account_resource_audit(audit(12), ContextDispatch::Scheduled)
            .expect("scheduled retry");
        assert_eq!(
            coordinator
                .settle_resource_audit(ContextResourceAuditSettlement::new(
                    audit(12),
                    Ok(zero_snapshot()),
                ))
                .expect("zero audit")
                .stage(),
            AgentNativeShutdownStage::ZeroProven
        );

        let proof = coordinator.finish().expect("unique zero proof");
        assert_eq!(proof.audit(), audit(12));
        assert_eq!(proof.attempts(), 3);
        assert!(native_snapshot_is_zero(proof.snapshot()));
        assert!(std::mem::size_of_val(&proof) <= MAX_AGENT_NATIVE_SHUTDOWN_PROOF_BYTES);
    }

    #[test]
    fn mismatched_or_replayed_audit_cannot_advance_current_debt() {
        let mut coordinator = coordinator();
        coordinator.begin_port_seal(audit(20)).expect("begin seal");
        assert_eq!(
            coordinator.account_port_seal(audit(21), ContextShutdownDispatch::AuditScheduled,),
            Err(AgentNativeShutdownError::AuditMismatch)
        );
        coordinator
            .account_port_seal(audit(20), ContextShutdownDispatch::AuditScheduled)
            .expect("correct seal result");
        assert_eq!(
            coordinator.settle_shutdown_audit(ContextShutdownAuditSettlement::new(
                audit(21),
                Ok(zero_snapshot()),
            )),
            Err(AgentNativeShutdownError::AuditMismatch)
        );
        coordinator
            .settle_shutdown_audit(ContextShutdownAuditSettlement::new(
                audit(20),
                Err(ContextPortFailure::NativeRefused),
            ))
            .expect("correct barrier settlement");
        assert_eq!(
            coordinator.begin_resource_audit(audit(20)),
            Err(AgentNativeShutdownError::AuditReplay)
        );
        coordinator
            .begin_resource_audit(audit(21))
            .expect("strictly newer audit");
        assert_eq!(
            coordinator
                .finish()
                .expect_err("pending audit is not zero proof")
                .into_coordinator()
                .status()
                .stage(),
            AgentNativeShutdownStage::ResourceAuditDispatchPending
        );
    }

    #[test]
    fn all_native_count_classes_must_be_zero() {
        let nonzero = [
            ContextNativeResourceCounts {
                known_bindings: 1,
                resident_views: 1,
                owned_reservations: 1,
                borrowed_leases: 0,
                visible_surfaces: 0,
                suspended_views: 0,
                pending_operations: 1,
                pending_captures: 0,
                queued_tasks: 0,
            },
            ContextNativeResourceCounts {
                known_bindings: 1,
                resident_views: 1,
                owned_reservations: 0,
                borrowed_leases: 1,
                visible_surfaces: 1,
                suspended_views: 1,
                pending_operations: 0,
                pending_captures: 0,
                queued_tasks: 0,
            },
            ContextNativeResourceCounts {
                known_bindings: 0,
                resident_views: 0,
                owned_reservations: 0,
                borrowed_leases: 0,
                visible_surfaces: 0,
                suspended_views: 0,
                pending_operations: 0,
                pending_captures: 1,
                queued_tasks: 0,
            },
            ContextNativeResourceCounts {
                known_bindings: 0,
                resident_views: 0,
                owned_reservations: 0,
                borrowed_leases: 0,
                visible_surfaces: 0,
                suspended_views: 0,
                pending_operations: 0,
                pending_captures: 0,
                queued_tasks: 1,
            },
        ];
        for counts in nonzero {
            let mut coordinator = coordinator();
            coordinator.begin_port_seal(audit(1)).expect("begin seal");
            coordinator
                .account_port_seal(audit(1), ContextShutdownDispatch::AuditScheduled)
                .expect("account seal");
            assert_eq!(
                coordinator
                    .settle_shutdown_audit(ContextShutdownAuditSettlement::new(
                        audit(1),
                        Ok(snapshot(counts)),
                    ))
                    .expect("settle nonzero")
                    .stage(),
                AgentNativeShutdownStage::ResourceAuditRequired
            );
        }
    }

    #[test]
    fn audit_attempts_are_hard_bounded_after_the_permanent_seal() {
        let mut coordinator = coordinator();
        coordinator.begin_port_seal(audit(1)).expect("begin seal");
        coordinator
            .account_port_seal(
                audit(1),
                ContextShutdownDispatch::SealedWithoutAudit(ContextPortFailure::ResourceExhausted),
            )
            .expect("sealed without audit");
        for raw in 2..=u64::from(MAX_AGENT_NATIVE_SHUTDOWN_AUDITS) {
            coordinator
                .begin_resource_audit(audit(raw))
                .expect("bounded retry");
            coordinator
                .account_resource_audit(
                    audit(raw),
                    ContextDispatch::Rejected(ContextPortFailure::ResourceExhausted),
                )
                .expect("bounded refusal");
        }
        let status = coordinator.status();
        assert_eq!(status.stage(), AgentNativeShutdownStage::Exhausted);
        assert_eq!(status.attempts(), MAX_AGENT_NATIVE_SHUTDOWN_AUDITS);
        assert_eq!(status.attempts_remaining(), 0);
        assert_eq!(
            coordinator.begin_resource_audit(audit(100)),
            Err(AgentNativeShutdownError::Stage)
        );
    }
}
