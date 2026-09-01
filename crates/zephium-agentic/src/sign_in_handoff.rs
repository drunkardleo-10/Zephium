//! Exact sign-in handoff workflow for extension-free owned contexts.
//!
//! This functional core coordinates existing context, profile-lease, native
//! port, and cookie-transfer contracts. It creates no page, task, timer, or
//! worker and carries no credentials, cookie values, page data, or native
//! handles. A shell actor remains responsible for executing each admitted
//! transition and settling both underlying registries before advancing this
//! workflow.

use std::fmt;
use std::num::NonZeroU64;

use thiserror::Error;

use crate::{
    ContextConstructionProof, ContextConstructionRequest, ContextConstructionSettlement,
    ContextCookieOrigin, ContextCookieScope, ContextCookieTransferDirection,
    ContextCookieTransferOutcome, ContextCookieTransferRequest, ContextCookieTransferSettlement,
    ContextId, ContextJoin, ContextKind, ContextNavigationRequest, ContextNavigationSettlement,
    ContextOperationJoin, ContextOperationKind, ContextResourceDisposition, ContextTerminal,
    ContextTransitionRequest, ContextTransitionSettlement, RetiredContext,
};

/// Nonzero process-local identity for one explicit sign-in handoff workflow.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContextSignInHandoffId(NonZeroU64);

impl ContextSignInHandoffId {
    /// Constructs a shell-minted nonzero workflow identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local correlation value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for ContextSignInHandoffId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ContextSignInHandoffId([redacted])")
    }
}

/// Platform storage strategy for one sign-in handoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextSignInHandoffPlatform {
    /// Extension-free and normal contexts share the selected WKWebsiteDataStore.
    MacOsSharedProfile,
    /// Normal profile state moves one way into a clean WebView2 automation subprofile.
    WindowsCookieBridge,
}

/// Typed blocker after a terminal native refusal; retry is a policy decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextSignInHandoffBlocker {
    /// Temporary normal context could not be constructed truthfully.
    HandoffConstruction,
    /// Native presentation for exclusive human input failed.
    HumanPresentation,
    /// Native return from exclusive human input failed.
    AgentReturn,
    /// Cookie transfer refused before destination mutation.
    CookieTransfer,
    /// Cookie transfer partially mutated the destination; it must be recreated.
    DestinationContaminated,
    /// Temporary handoff release failed.
    HandoffRelease,
    /// Owned-context refresh/navigation failed.
    OwnedRefresh,
    /// Owned refresh committed outside the explicitly bounded origin scope.
    RefreshOutsideScope,
}

/// Observable workflow phase without URLs, profiles, or context identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextSignInHandoffState {
    /// Temporary normal context construction is in flight.
    ConstructingHandoff,
    /// Construction succeeded; exclusive human presentation may begin.
    AwaitingHumanControl,
    /// Native presentation for exclusive human input is in flight.
    PresentingToHuman,
    /// A person exclusively controls the temporary normal context.
    HumanControl,
    /// The person declared sign-in complete; input return may begin.
    AwaitingAgentReturn,
    /// Native return from exclusive human input is in flight.
    ReturningToAgent,
    /// Windows one-way cookie transfer may begin.
    AwaitingCookieTransfer,
    /// Windows one-way cookie transfer is in flight.
    TransferringCookies,
    /// Temporary normal context may be released.
    AwaitingHandoffRelease,
    /// Native release of the temporary normal context is in flight.
    ReleasingHandoff,
    /// Owned context must refresh/navigate against the resulting storage state.
    AwaitingOwnedRefresh,
    /// Exact owned-context refresh/navigation is in flight.
    RefreshingOwned,
    /// Complete fresh semantic observation is required.
    AwaitingOwnedObservation,
    /// Fresh owned context is ready and temporary handoff is released.
    Complete,
    /// Workflow stopped after a typed native refusal or unsafe partial effect.
    Blocked(ContextSignInHandoffBlocker),
    /// Run cancellation is draining owned close and handoff release.
    Cancelling,
    /// Both cancellation cleanup obligations reached terminal dispositions.
    Cancelled,
}

/// Closed workflow transition refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContextSignInHandoffError {
    /// Owned or handoff context kind is incompatible.
    #[error("sign-in handoff context kind is incompatible")]
    ContextKind,
    /// Owned and handoff contexts belong to different runs.
    #[error("sign-in handoff owner does not match")]
    OwnerMismatch,
    /// Owned and handoff contexts selected different profiles.
    #[error("sign-in handoff profile does not match")]
    ProfileMismatch,
    /// Owned and handoff identities must be distinct.
    #[error("sign-in handoff contexts must be distinct")]
    SameContext,
    /// Construction request source is not a human sign-in handoff.
    #[error("sign-in handoff construction source is incompatible")]
    ConstructionSource,
    /// Platform storage/extension proof is incompatible with this workflow.
    #[error("sign-in handoff construction proof is incompatible")]
    ConstructionProof,
    /// Workflow phase does not admit this transition.
    #[error("invalid sign-in handoff workflow transition")]
    InvalidState,
    /// Operation class is incompatible with the workflow transition.
    #[error("sign-in handoff operation kind is incompatible")]
    OperationKind,
    /// Operation does not directly follow the exact retained context join.
    #[error("sign-in handoff operation join is stale")]
    StaleJoin,
    /// Settlement does not match the exact admitted operation or transfer.
    #[error("sign-in handoff settlement is stale")]
    StaleSettlement,
    /// Cookie-transfer direction, endpoints, or origin scope is incompatible.
    #[error("sign-in handoff cookie transfer is incompatible")]
    CookieTransfer,
    /// Owned refresh committed outside the bounded requested origin scope.
    #[error("sign-in handoff refresh target is outside scope")]
    RefreshTarget,
    /// Completion lacks a fresh automatable owned-context observation.
    #[error("sign-in handoff owned observation is not fresh")]
    ObservationRequired,
    /// Cancellation cleanup reported the wrong terminal disposition.
    #[error("sign-in handoff cleanup disposition is incompatible")]
    CleanupDisposition,
}

/// Exact cleanup identities after cancellation seals the workflow.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ContextSignInHandoffCleanup {
    owned: ContextId,
    handoff: ContextId,
    destination_contaminated: bool,
}

impl ContextSignInHandoffCleanup {
    /// Owned context that must close.
    pub const fn owned(self) -> ContextId {
        self.owned
    }

    /// Temporary normal context that must release.
    pub const fn handoff(self) -> ContextId {
        self.handoff
    }

    /// Whether partial cookie application requires destination recreation.
    pub const fn destination_contaminated(self) -> bool {
        self.destination_contaminated
    }
}

impl fmt::Debug for ContextSignInHandoffCleanup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSignInHandoffCleanup")
            .field("owned", &"[redacted]")
            .field("handoff", &"[redacted]")
            .field("destination_contaminated", &self.destination_contaminated)
            .finish()
    }
}

/// Pure exact-join state machine for one explicit sign-in handoff.
pub struct ContextSignInHandoff {
    id: ContextSignInHandoffId,
    platform: ContextSignInHandoffPlatform,
    owned: ContextJoin,
    owned_proof: ContextConstructionProof,
    handoff_identity: crate::ContextIdentity,
    handoff: Option<ContextJoin>,
    handoff_proof: Option<ContextConstructionProof>,
    scope: ContextCookieScope,
    state: ContextSignInHandoffState,
    pending_operation: Option<ContextOperationJoin>,
    pending_transfer: Option<ContextCookieTransferRequest>,
    owned_cleanup_complete: bool,
    handoff_cleanup_complete: bool,
    destination_contaminated: bool,
}

impl ContextSignInHandoff {
    /// Begins with an already-admitted temporary-context construction request.
    pub fn try_new(
        id: ContextSignInHandoffId,
        platform: ContextSignInHandoffPlatform,
        owned: ContextJoin,
        owned_proof: ContextConstructionProof,
        handoff_construction: ContextConstructionRequest,
        scope: ContextCookieScope,
    ) -> Result<Self, ContextSignInHandoffError> {
        if owned.identity().kind() != ContextKind::Owned
            || handoff_construction.operation().context().identity().kind()
                != ContextKind::HumanSignInHandoff
        {
            return Err(ContextSignInHandoffError::ContextKind);
        }
        if !matches!(
            handoff_construction.source(),
            crate::ContextConstructionSource::HumanSignInHandoff
        ) {
            return Err(ContextSignInHandoffError::ConstructionSource);
        }
        let handoff_identity = handoff_construction.operation().context().identity();
        validate_endpoint_pair(owned, handoff_identity)?;
        validate_owned_proof(platform, owned_proof)?;
        Ok(Self {
            id,
            platform,
            owned,
            owned_proof,
            handoff_identity,
            handoff: None,
            handoff_proof: None,
            scope,
            state: ContextSignInHandoffState::ConstructingHandoff,
            pending_operation: Some(handoff_construction.operation()),
            pending_transfer: None,
            owned_cleanup_complete: false,
            handoff_cleanup_complete: false,
            destination_contaminated: false,
        })
    }

    /// Process-local workflow identity.
    pub const fn id(&self) -> ContextSignInHandoffId {
        self.id
    }

    /// Current redaction-safe workflow phase.
    pub const fn state(&self) -> ContextSignInHandoffState {
        self.state
    }

    /// Exact current owned-context join.
    pub const fn owned(&self) -> ContextJoin {
        self.owned
    }

    /// Exact current handoff join after successful construction.
    pub const fn handoff(&self) -> Option<ContextJoin> {
        self.handoff
    }

    /// Settles initial temporary normal context construction.
    pub fn settle_handoff_construction(
        &mut self,
        settlement: ContextConstructionSettlement,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::ConstructingHandoff)?;
        self.require_pending(settlement.operation())?;
        self.pending_operation = None;
        match settlement.outcome() {
            Ok(proof) => {
                if validate_handoff_proof(self.platform, proof).is_ok() {
                    self.handoff = Some(settlement.operation().context());
                    self.handoff_proof = Some(proof);
                    self.state = ContextSignInHandoffState::AwaitingHumanControl;
                } else {
                    self.state = ContextSignInHandoffState::Blocked(
                        ContextSignInHandoffBlocker::HandoffConstruction,
                    );
                }
            }
            Err(_) => {
                self.state = ContextSignInHandoffState::Blocked(
                    ContextSignInHandoffBlocker::HandoffConstruction,
                );
            }
        }
        Ok(())
    }

    /// Admits exact transfer of exclusive input to a person.
    pub fn begin_human_control(
        &mut self,
        request: ContextTransitionRequest,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::AwaitingHumanControl)?;
        self.require_handoff_successor(
            request.operation(),
            ContextOperationKind::BeginHumanControl,
        )?;
        self.pending_operation = Some(request.operation());
        self.state = ContextSignInHandoffState::PresentingToHuman;
        Ok(())
    }

    /// Settles native human presentation; input is never returned on refusal.
    pub fn settle_human_control(
        &mut self,
        settlement: ContextTransitionSettlement,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::PresentingToHuman)?;
        self.require_pending(settlement.operation())?;
        self.pending_operation = None;
        self.handoff = Some(settlement.operation().context());
        self.state = if settlement.outcome().is_ok() {
            ContextSignInHandoffState::HumanControl
        } else {
            ContextSignInHandoffState::Blocked(ContextSignInHandoffBlocker::HumanPresentation)
        };
        Ok(())
    }

    /// Records an explicit human declaration that sign-in is complete.
    pub fn confirm_human_sign_in(&mut self) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::HumanControl)?;
        self.state = ContextSignInHandoffState::AwaitingAgentReturn;
        Ok(())
    }

    /// Admits exact return from exclusive human input.
    pub fn begin_agent_return(
        &mut self,
        request: ContextTransitionRequest,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::AwaitingAgentReturn)?;
        self.require_handoff_successor(request.operation(), ContextOperationKind::EndHumanControl)?;
        self.pending_operation = Some(request.operation());
        self.state = ContextSignInHandoffState::ReturningToAgent;
        Ok(())
    }

    /// Settles return from human input and selects the platform storage step.
    pub fn settle_agent_return(
        &mut self,
        settlement: ContextTransitionSettlement,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::ReturningToAgent)?;
        self.require_pending(settlement.operation())?;
        self.pending_operation = None;
        self.handoff = Some(settlement.operation().context());
        self.state = if settlement.outcome().is_err() {
            ContextSignInHandoffState::Blocked(ContextSignInHandoffBlocker::AgentReturn)
        } else {
            match self.platform {
                ContextSignInHandoffPlatform::MacOsSharedProfile => {
                    ContextSignInHandoffState::AwaitingHandoffRelease
                }
                ContextSignInHandoffPlatform::WindowsCookieBridge => {
                    ContextSignInHandoffState::AwaitingCookieTransfer
                }
            }
        };
        Ok(())
    }

    /// Admits the exact Windows handoff-to-owned cookie transfer.
    pub fn begin_cookie_transfer(
        &mut self,
        request: ContextCookieTransferRequest,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::AwaitingCookieTransfer)?;
        if request.direction() != ContextCookieTransferDirection::HumanHandoffToOwned
            || request.source() != self.handoff
            || request.destination() != self.owned
            || request.scope() != &self.scope
        {
            return Err(ContextSignInHandoffError::CookieTransfer);
        }
        self.pending_transfer = Some(request);
        self.state = ContextSignInHandoffState::TransferringCookies;
        Ok(())
    }

    /// Settles Windows cookie transfer without converting partial writes to success.
    pub fn settle_cookie_transfer(
        &mut self,
        settlement: &ContextCookieTransferSettlement,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::TransferringCookies)?;
        let pending = self
            .pending_transfer
            .as_ref()
            .ok_or(ContextSignInHandoffError::StaleSettlement)?;
        if pending != settlement.request() {
            return Err(ContextSignInHandoffError::StaleSettlement);
        }
        self.pending_transfer = None;
        self.state = match settlement.outcome() {
            ContextCookieTransferOutcome::Applied(_) => {
                ContextSignInHandoffState::AwaitingHandoffRelease
            }
            ContextCookieTransferOutcome::Refused(_) => {
                ContextSignInHandoffState::Blocked(ContextSignInHandoffBlocker::CookieTransfer)
            }
            ContextCookieTransferOutcome::Partial { .. } => {
                self.destination_contaminated = true;
                ContextSignInHandoffState::Blocked(
                    ContextSignInHandoffBlocker::DestinationContaminated,
                )
            }
        };
        Ok(())
    }

    /// Admits exact release of the temporary normal context.
    pub fn begin_handoff_release(
        &mut self,
        request: ContextTransitionRequest,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::AwaitingHandoffRelease)?;
        self.require_handoff_successor(request.operation(), ContextOperationKind::Release)?;
        self.pending_operation = Some(request.operation());
        self.state = ContextSignInHandoffState::ReleasingHandoff;
        Ok(())
    }

    /// Settles temporary-context release before owned refresh can proceed.
    pub fn settle_handoff_release(
        &mut self,
        settlement: ContextTransitionSettlement,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::ReleasingHandoff)?;
        self.require_pending(settlement.operation())?;
        self.pending_operation = None;
        self.handoff = Some(settlement.operation().context());
        self.state = if settlement.outcome().is_ok() {
            ContextSignInHandoffState::AwaitingOwnedRefresh
        } else {
            ContextSignInHandoffState::Blocked(ContextSignInHandoffBlocker::HandoffRelease)
        };
        Ok(())
    }

    /// Admits one exact owned navigation whose origin was explicitly scoped.
    pub fn begin_owned_refresh(
        &mut self,
        request: &ContextNavigationRequest,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::AwaitingOwnedRefresh)?;
        require_navigation_successor(self.owned, request.operation())?;
        let origin = ContextCookieOrigin::from_target(request.target())
            .map_err(|_| ContextSignInHandoffError::RefreshTarget)?;
        if !self.scope.origins().contains(&origin) {
            return Err(ContextSignInHandoffError::RefreshTarget);
        }
        self.pending_operation = Some(request.operation());
        self.state = ContextSignInHandoffState::RefreshingOwned;
        Ok(())
    }

    /// Settles the owned refresh and requires its actual commit to remain scoped.
    pub fn settle_owned_refresh(
        &mut self,
        settlement: &ContextNavigationSettlement,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::RefreshingOwned)?;
        self.require_pending(settlement.operation())?;
        self.pending_operation = None;
        self.owned = settlement.operation().context();
        match settlement.outcome() {
            Ok(target) => {
                let origin = ContextCookieOrigin::from_target(target).ok();
                if !origin
                    .as_ref()
                    .is_some_and(|origin| self.scope.origins().contains(origin))
                {
                    self.state = ContextSignInHandoffState::Blocked(
                        ContextSignInHandoffBlocker::RefreshOutsideScope,
                    );
                    return Ok(());
                }
                self.state = ContextSignInHandoffState::AwaitingOwnedObservation;
            }
            Err(_) => {
                self.state =
                    ContextSignInHandoffState::Blocked(ContextSignInHandoffBlocker::OwnedRefresh);
            }
        }
        Ok(())
    }

    /// Completes only after the shell proves a fresh exact owned observation.
    pub fn complete_after_observation(
        &mut self,
        observed: ContextJoin,
        entry: crate::ContextRegistryEntry,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::AwaitingOwnedObservation)?;
        let crate::ContextRegistryEntryState::Active(status) = entry.state() else {
            return Err(ContextSignInHandoffError::ObservationRequired);
        };
        if observed != self.owned
            || entry.identity() != self.owned.identity()
            || !status.can_automate()
        {
            return Err(ContextSignInHandoffError::ObservationRequired);
        }
        self.state = ContextSignInHandoffState::Complete;
        Ok(())
    }

    /// Seals any nonterminal workflow and returns exact cleanup obligations.
    pub fn begin_cancellation(
        &mut self,
    ) -> Result<ContextSignInHandoffCleanup, ContextSignInHandoffError> {
        if matches!(
            self.state,
            ContextSignInHandoffState::Complete
                | ContextSignInHandoffState::Cancelling
                | ContextSignInHandoffState::Cancelled
        ) {
            return Err(ContextSignInHandoffError::InvalidState);
        }
        self.pending_operation = None;
        self.pending_transfer = None;
        self.state = ContextSignInHandoffState::Cancelling;
        Ok(ContextSignInHandoffCleanup {
            owned: self.owned.identity().id(),
            handoff: self.handoff_identity.id(),
            destination_contaminated: self.destination_contaminated,
        })
    }

    /// Records one exact terminal context disposition during cancellation drain.
    pub fn observe_cleanup_terminal(
        &mut self,
        retired: RetiredContext,
    ) -> Result<(), ContextSignInHandoffError> {
        self.require_state(ContextSignInHandoffState::Cancelling)?;
        let context = retired.identity().id();
        if context == self.owned.identity().id() {
            if retired.terminal() != ContextTerminal::Closed
                || retired.resource() != ContextResourceDisposition::Destroyed
            {
                return Err(ContextSignInHandoffError::CleanupDisposition);
            }
            self.owned_cleanup_complete = true;
        } else if context == self.handoff_identity.id() {
            if retired.terminal() != ContextTerminal::Released
                || retired.resource() != ContextResourceDisposition::Destroyed
            {
                return Err(ContextSignInHandoffError::CleanupDisposition);
            }
            self.handoff_cleanup_complete = true;
        } else {
            return Err(ContextSignInHandoffError::CleanupDisposition);
        }
        if self.owned_cleanup_complete && self.handoff_cleanup_complete {
            self.state = ContextSignInHandoffState::Cancelled;
        }
        Ok(())
    }

    fn require_state(
        &self,
        expected: ContextSignInHandoffState,
    ) -> Result<(), ContextSignInHandoffError> {
        if self.state == expected {
            Ok(())
        } else {
            Err(ContextSignInHandoffError::InvalidState)
        }
    }

    fn require_pending(
        &self,
        operation: ContextOperationJoin,
    ) -> Result<(), ContextSignInHandoffError> {
        if self.pending_operation == Some(operation) {
            Ok(())
        } else {
            Err(ContextSignInHandoffError::StaleSettlement)
        }
    }

    fn require_handoff_successor(
        &self,
        operation: ContextOperationJoin,
        kind: ContextOperationKind,
    ) -> Result<(), ContextSignInHandoffError> {
        if operation.kind() != kind {
            return Err(ContextSignInHandoffError::OperationKind);
        }
        let current = self.handoff.ok_or(ContextSignInHandoffError::StaleJoin)?;
        require_full_successor(current, operation.context())
    }
}

impl fmt::Debug for ContextSignInHandoff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSignInHandoff")
            .field("id", &self.id)
            .field("platform", &self.platform)
            .field("owned", &self.owned)
            .field("owned_proof", &self.owned_proof)
            .field("handoff_identity", &self.handoff_identity)
            .field("handoff", &self.handoff)
            .field("handoff_proof", &self.handoff_proof)
            .field("scope", &self.scope)
            .field("state", &self.state)
            .field("destination_contaminated", &self.destination_contaminated)
            .finish()
    }
}

fn validate_endpoint_pair(
    owned: ContextJoin,
    handoff: crate::ContextIdentity,
) -> Result<(), ContextSignInHandoffError> {
    if owned.identity().id() == handoff.id() {
        return Err(ContextSignInHandoffError::SameContext);
    }
    if owned.identity().owner() != handoff.owner() {
        return Err(ContextSignInHandoffError::OwnerMismatch);
    }
    if owned.identity().profile() != handoff.profile() {
        return Err(ContextSignInHandoffError::ProfileMismatch);
    }
    Ok(())
}

fn validate_owned_proof(
    platform: ContextSignInHandoffPlatform,
    proof: ContextConstructionProof,
) -> Result<(), ContextSignInHandoffError> {
    let expected = match platform {
        ContextSignInHandoffPlatform::MacOsSharedProfile => {
            ContextConstructionProof::MacOsOwnedSelectedProfileExtensionFree
        }
        ContextSignInHandoffPlatform::WindowsCookieBridge => {
            ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory
        }
    };
    if proof == expected {
        Ok(())
    } else {
        Err(ContextSignInHandoffError::ConstructionProof)
    }
}

fn validate_handoff_proof(
    platform: ContextSignInHandoffPlatform,
    proof: ContextConstructionProof,
) -> Result<(), ContextSignInHandoffError> {
    let expected = match platform {
        ContextSignInHandoffPlatform::MacOsSharedProfile => {
            ContextConstructionProof::MacOsHumanSignInHandoffNormalExtensions
        }
        ContextSignInHandoffPlatform::WindowsCookieBridge => {
            ContextConstructionProof::WindowsHumanSignInHandoffNormalExtensions
        }
    };
    if proof == expected {
        Ok(())
    } else {
        Err(ContextSignInHandoffError::ConstructionProof)
    }
}

fn require_full_successor(
    current: ContextJoin,
    successor: ContextJoin,
) -> Result<(), ContextSignInHandoffError> {
    if current.identity() == successor.identity()
        && current.context_generation().next() == Some(successor.context_generation())
        && current.navigation_epoch().next() == Some(successor.navigation_epoch())
        && current.frame() == successor.frame()
        && current.frame_generation().next() == Some(successor.frame_generation())
        && current.cancellation_generation().next() == Some(successor.cancellation_generation())
    {
        Ok(())
    } else {
        Err(ContextSignInHandoffError::StaleJoin)
    }
}

fn require_navigation_successor(
    current: ContextJoin,
    operation: ContextOperationJoin,
) -> Result<(), ContextSignInHandoffError> {
    let successor = operation.context();
    if operation.kind() == ContextOperationKind::Navigate
        && current.identity() == successor.identity()
        && current.context_generation() == successor.context_generation()
        && current.navigation_epoch().next() == Some(successor.navigation_epoch())
        && current.frame() == successor.frame()
        && current.frame_generation().next() == Some(successor.frame_generation())
        && current.cancellation_generation() == successor.cancellation_generation()
    {
        Ok(())
    } else if operation.kind() != ContextOperationKind::Navigate {
        Err(ContextSignInHandoffError::OperationKind)
    } else {
        Err(ContextSignInHandoffError::StaleJoin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextCapabilities, ContextCapability, ContextConstructionSource,
        ContextCookieTransferCounts, ContextCookieTransferFailure, ContextCookieTransferId,
        ContextCookieTransferStats, ContextIdentity, ContextNavigationTarget, ContextOperationId,
        ContextProfileLeaseId, ContextProfileLeasePurpose, ContextProfileLeaseRegistry,
        ContextProfileStorageClass, ContextRegistry, ContextRunId, ContextSettlement,
    };
    use zephium_core::ids::ProfileId;

    const OWNED_CAPABILITIES: [ContextCapability; 4] = [
        ContextCapability::Navigate,
        ContextCapability::Observe,
        ContextCapability::ImportCookies,
        ContextCapability::Act,
    ];
    const HANDOFF_CAPABILITIES: [ContextCapability; 5] = [
        ContextCapability::Navigate,
        ContextCapability::Present,
        ContextCapability::HumanControl,
        ContextCapability::ExportCookies,
        ContextCapability::Release,
    ];

    fn capabilities(kind: ContextKind) -> ContextCapabilities {
        let values = match kind {
            ContextKind::Owned => &OWNED_CAPABILITIES[..],
            ContextKind::HumanSignInHandoff => &HANDOFF_CAPABILITIES[..],
            ContextKind::BorrowedTab => &[][..],
        };
        ContextCapabilities::try_new(kind, values).expect("capabilities")
    }

    fn identity(context: u128, kind: ContextKind) -> ContextIdentity {
        ContextIdentity::new(
            ContextId::from_raw(context),
            ContextRunId::from_raw(10),
            ProfileId::from(20),
            kind,
        )
    }

    fn operation(value: u64) -> ContextOperationId {
        ContextOperationId::new(value).expect("operation")
    }

    fn profile_lease(identity: ContextIdentity, value: u64) -> crate::ContextProfileLease {
        let purpose = match identity.kind() {
            ContextKind::Owned => ContextProfileLeasePurpose::Owned,
            ContextKind::BorrowedTab => ContextProfileLeasePurpose::BorrowedTab,
            ContextKind::HumanSignInHandoff => ContextProfileLeasePurpose::HumanSignInHandoff,
        };
        ContextProfileLeaseRegistry::new()
            .acquire(
                ContextProfileLeaseId::new(value).expect("lease"),
                identity,
                ContextProfileStorageClass::Durable,
                purpose,
            )
            .expect("profile lease")
    }

    struct Fixture {
        registry: ContextRegistry,
        workflow: ContextSignInHandoff,
        owned_lease: crate::ContextProfileLease,
        handoff_lease: crate::ContextProfileLease,
    }

    fn fixture(platform: ContextSignInHandoffPlatform) -> Fixture {
        let owned = identity(1, ContextKind::Owned);
        let handoff = identity(2, ContextKind::HumanSignInHandoff);
        let mut registry = ContextRegistry::new();
        registry
            .reserve(owned, capabilities(ContextKind::Owned))
            .expect("reserve owned");
        registry
            .reserve(handoff, capabilities(ContextKind::HumanSignInHandoff))
            .expect("reserve handoff");
        let owned_construction = registry
            .begin_context(owned.id(), operation(1))
            .expect("construct owned");
        registry
            .settle_construction(owned.id(), owned_construction, ContextSettlement::Applied)
            .expect("settle owned");
        let owned_join = registry.join(owned.id()).expect("owned join");
        let handoff_construction = registry
            .begin_context(handoff.id(), operation(2))
            .expect("construct handoff");
        let owned_lease = profile_lease(owned, 1);
        let handoff_lease = profile_lease(handoff, 2);
        let request = ContextConstructionRequest::try_new(
            handoff_construction,
            capabilities(ContextKind::HumanSignInHandoff),
            handoff_lease,
            ContextConstructionSource::HumanSignInHandoff,
        )
        .expect("construction request");
        let owned_proof = match platform {
            ContextSignInHandoffPlatform::MacOsSharedProfile => {
                ContextConstructionProof::MacOsOwnedSelectedProfileExtensionFree
            }
            ContextSignInHandoffPlatform::WindowsCookieBridge => {
                ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory
            }
        };
        let scope = ContextCookieScope::try_new(vec![ContextCookieOrigin::parse(
            "https://example.test/login",
        )
        .expect("origin")])
        .expect("scope");
        let workflow = ContextSignInHandoff::try_new(
            ContextSignInHandoffId::new(1).expect("workflow"),
            platform,
            owned_join,
            owned_proof,
            request,
            scope,
        )
        .expect("workflow");
        Fixture {
            registry,
            workflow,
            owned_lease,
            handoff_lease,
        }
    }

    fn construct_handoff(fixture: &mut Fixture, platform: ContextSignInHandoffPlatform) {
        let operation = fixture.workflow.pending_operation.expect("construction");
        let proof = match platform {
            ContextSignInHandoffPlatform::MacOsSharedProfile => {
                ContextConstructionProof::MacOsHumanSignInHandoffNormalExtensions
            }
            ContextSignInHandoffPlatform::WindowsCookieBridge => {
                ContextConstructionProof::WindowsHumanSignInHandoffNormalExtensions
            }
        };
        fixture
            .registry
            .settle_construction(
                operation.context().identity().id(),
                operation,
                ContextSettlement::Applied,
            )
            .expect("settle registry construction");
        fixture
            .workflow
            .settle_handoff_construction(
                ContextConstructionSettlement::try_new(operation, Ok(proof)).expect("settlement"),
            )
            .expect("settle workflow construction");
    }

    fn perform_human_control(fixture: &mut Fixture) {
        let handoff = fixture.workflow.handoff_identity.id();
        let begin = fixture
            .registry
            .begin_human_control(handoff, operation(3))
            .expect("begin human");
        fixture
            .workflow
            .begin_human_control(ContextTransitionRequest::try_new(begin).expect("request"))
            .expect("workflow begin human");
        fixture
            .registry
            .settle_begin_human_control(handoff, begin, ContextSettlement::Applied)
            .expect("settle human");
        fixture
            .workflow
            .settle_human_control(
                ContextTransitionSettlement::try_new(begin, Ok(())).expect("settlement"),
            )
            .expect("workflow settle human");
        fixture
            .workflow
            .confirm_human_sign_in()
            .expect("confirm sign in");
        let end = fixture
            .registry
            .begin_end_human_control(handoff, operation(4))
            .expect("end human");
        fixture
            .workflow
            .begin_agent_return(ContextTransitionRequest::try_new(end).expect("request"))
            .expect("workflow begin return");
        fixture
            .registry
            .settle_end_human_control(handoff, end, ContextSettlement::Applied)
            .expect("settle return");
        fixture
            .workflow
            .settle_agent_return(
                ContextTransitionSettlement::try_new(end, Ok(())).expect("settlement"),
            )
            .expect("workflow settle return");
    }

    fn release_handoff(fixture: &mut Fixture) {
        let handoff = fixture.workflow.handoff_identity.id();
        let release = fixture
            .registry
            .begin_release(handoff, operation(6))
            .expect("release");
        fixture
            .workflow
            .begin_handoff_release(ContextTransitionRequest::try_new(release).expect("request"))
            .expect("workflow release");
        fixture
            .registry
            .settle_release(handoff, release, ContextSettlement::Applied)
            .expect("settle release");
        fixture
            .workflow
            .settle_handoff_release(
                ContextTransitionSettlement::try_new(release, Ok(())).expect("settlement"),
            )
            .expect("workflow settle release");
    }

    fn refresh_and_observe_owned(fixture: &mut Fixture) {
        let owned = fixture.workflow.owned.identity().id();
        let navigation = fixture
            .registry
            .begin_navigation(owned, operation(7))
            .expect("navigation");
        let target =
            ContextNavigationTarget::parse("https://example.test/account").expect("target");
        let request =
            ContextNavigationRequest::try_new(navigation, target.clone()).expect("request");
        fixture
            .workflow
            .begin_owned_refresh(&request)
            .expect("workflow refresh");
        fixture
            .registry
            .settle_navigation(owned, navigation, ContextSettlement::Applied)
            .expect("settle navigation");
        fixture
            .workflow
            .settle_owned_refresh(
                &ContextNavigationSettlement::try_new(navigation, Ok(target)).expect("settlement"),
            )
            .expect("workflow settle refresh");
        let join = fixture.registry.join(owned).expect("join");
        fixture
            .registry
            .acknowledge_observation(owned, join)
            .expect("observation");
        let entry = fixture.registry.entry(owned).expect("entry");
        fixture
            .workflow
            .complete_after_observation(join, entry)
            .expect("complete");
    }

    #[test]
    fn windows_handoff_requires_transfer_release_refresh_and_fresh_observation() {
        let platform = ContextSignInHandoffPlatform::WindowsCookieBridge;
        let mut fixture = fixture(platform);
        construct_handoff(&mut fixture, platform);
        perform_human_control(&mut fixture);
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::AwaitingCookieTransfer
        );
        let transfer = ContextCookieTransferRequest::human_handoff_to_owned(
            ContextCookieTransferId::new(1).expect("transfer"),
            fixture.workflow.handoff.expect("handoff"),
            capabilities(ContextKind::HumanSignInHandoff),
            fixture.handoff_lease,
            ContextConstructionProof::WindowsHumanSignInHandoffNormalExtensions,
            fixture.workflow.owned,
            capabilities(ContextKind::Owned),
            fixture.owned_lease,
            ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory,
            fixture.workflow.scope.clone(),
        )
        .expect("transfer request");
        fixture
            .workflow
            .begin_cookie_transfer(transfer.clone())
            .expect("begin transfer");
        let stats = ContextCookieTransferStats::try_new(ContextCookieTransferCounts {
            origins_completed: 1,
            cookies_observed: 2,
            cookies_applied: 2,
            http_only_observed: 1,
            http_only_applied: 1,
            payload_bytes: 256,
        })
        .expect("stats");
        fixture
            .workflow
            .settle_cookie_transfer(
                &ContextCookieTransferSettlement::try_new(
                    transfer,
                    ContextCookieTransferOutcome::Applied(stats),
                )
                .expect("settlement"),
            )
            .expect("settle transfer");
        release_handoff(&mut fixture);
        refresh_and_observe_owned(&mut fixture);
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::Complete
        );
    }

    #[test]
    fn macos_shared_profile_skips_cookie_transfer_but_not_release_or_observation() {
        let platform = ContextSignInHandoffPlatform::MacOsSharedProfile;
        let mut fixture = fixture(platform);
        construct_handoff(&mut fixture, platform);
        perform_human_control(&mut fixture);
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::AwaitingHandoffRelease
        );
        release_handoff(&mut fixture);
        refresh_and_observe_owned(&mut fixture);
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::Complete
        );
    }

    #[test]
    fn incompatible_handoff_proof_terminally_blocks_instead_of_stalling() {
        let mut fixture = fixture(ContextSignInHandoffPlatform::MacOsSharedProfile);
        let operation = fixture.workflow.pending_operation.expect("construction");
        fixture
            .workflow
            .settle_handoff_construction(
                ContextConstructionSettlement::try_new(
                    operation,
                    Ok(ContextConstructionProof::WindowsHumanSignInHandoffNormalExtensions),
                )
                .expect("typed settlement"),
            )
            .expect("consume terminal settlement");
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::Blocked(ContextSignInHandoffBlocker::HandoffConstruction)
        );
    }

    #[test]
    fn committed_refresh_outside_scope_updates_join_and_blocks_completion() {
        let platform = ContextSignInHandoffPlatform::MacOsSharedProfile;
        let mut fixture = fixture(platform);
        construct_handoff(&mut fixture, platform);
        perform_human_control(&mut fixture);
        release_handoff(&mut fixture);
        let owned = fixture.workflow.owned.identity().id();
        let navigation = fixture
            .registry
            .begin_navigation(owned, operation(7))
            .expect("navigation");
        let requested =
            ContextNavigationTarget::parse("https://example.test/account").expect("target");
        fixture
            .workflow
            .begin_owned_refresh(
                &ContextNavigationRequest::try_new(navigation, requested).expect("request"),
            )
            .expect("begin refresh");
        let committed = ContextNavigationTarget::parse("https://unexpected.test/").expect("commit");
        fixture
            .workflow
            .settle_owned_refresh(
                &ContextNavigationSettlement::try_new(navigation, Ok(committed))
                    .expect("settlement"),
            )
            .expect("consume settlement");
        assert_eq!(fixture.workflow.owned(), navigation.context());
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::Blocked(ContextSignInHandoffBlocker::RefreshOutsideScope)
        );
    }

    #[test]
    fn partial_cookie_application_blocks_use_and_marks_cleanup_contaminated() {
        let platform = ContextSignInHandoffPlatform::WindowsCookieBridge;
        let mut fixture = fixture(platform);
        construct_handoff(&mut fixture, platform);
        perform_human_control(&mut fixture);
        let transfer = ContextCookieTransferRequest::human_handoff_to_owned(
            ContextCookieTransferId::new(1).expect("transfer"),
            fixture.workflow.handoff.expect("handoff"),
            capabilities(ContextKind::HumanSignInHandoff),
            fixture.handoff_lease,
            ContextConstructionProof::WindowsHumanSignInHandoffNormalExtensions,
            fixture.workflow.owned,
            capabilities(ContextKind::Owned),
            fixture.owned_lease,
            ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory,
            fixture.workflow.scope.clone(),
        )
        .expect("transfer");
        fixture
            .workflow
            .begin_cookie_transfer(transfer.clone())
            .expect("begin");
        let stats = ContextCookieTransferStats::try_new(ContextCookieTransferCounts {
            origins_completed: 0,
            cookies_observed: 2,
            cookies_applied: 1,
            http_only_observed: 1,
            http_only_applied: 1,
            payload_bytes: 128,
        })
        .expect("stats");
        fixture
            .workflow
            .settle_cookie_transfer(
                &ContextCookieTransferSettlement::try_new(
                    transfer,
                    ContextCookieTransferOutcome::Partial {
                        failure: ContextCookieTransferFailure::ApplicationFailed,
                        stats,
                    },
                )
                .expect("settlement"),
            )
            .expect("settle");
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::Blocked(
                ContextSignInHandoffBlocker::DestinationContaminated
            )
        );
        let cleanup = fixture.workflow.begin_cancellation().expect("cancellation");
        assert!(cleanup.destination_contaminated());
    }

    #[test]
    fn stale_or_wrong_class_operations_do_not_advance_workflow() {
        let platform = ContextSignInHandoffPlatform::MacOsSharedProfile;
        let mut fixture = fixture(platform);
        construct_handoff(&mut fixture, platform);
        let handoff = fixture.workflow.handoff_identity.id();
        let show = fixture
            .registry
            .begin_show(handoff, operation(3))
            .expect("show");
        assert_eq!(
            fixture
                .workflow
                .begin_human_control(ContextTransitionRequest::try_new(show).expect("request")),
            Err(ContextSignInHandoffError::OperationKind)
        );
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::AwaitingHumanControl
        );
    }

    #[test]
    fn cancellation_requires_owned_close_and_handoff_release_terminal_facts() {
        let platform = ContextSignInHandoffPlatform::MacOsSharedProfile;
        let mut fixture = fixture(platform);
        let cleanup = fixture.workflow.begin_cancellation().expect("cancellation");
        assert!(!format!("{cleanup:?}").contains("ProfileId"));
        let owned_prior = fixture.registry.join(cleanup.owned()).expect("owned join");
        fixture
            .registry
            .cancel_run(cleanup.owned(), owned_prior)
            .expect("cancel owned");
        let close = fixture
            .registry
            .begin_close(cleanup.owned(), operation(8))
            .expect("close owned");
        fixture
            .registry
            .settle_close(cleanup.owned(), close, ContextSettlement::Applied)
            .expect("settle close");
        let retired_owned = fixture
            .registry
            .reap_terminal(cleanup.owned())
            .expect("reap owned");
        fixture
            .workflow
            .observe_cleanup_terminal(retired_owned)
            .expect("owned closed");
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::Cancelling
        );
        let handoff_prior = fixture
            .registry
            .join(cleanup.handoff())
            .expect("handoff join");
        fixture
            .registry
            .cancel_run(cleanup.handoff(), handoff_prior)
            .expect("cancel handoff");
        let release = fixture
            .registry
            .begin_release(cleanup.handoff(), operation(9))
            .expect("release handoff");
        fixture
            .registry
            .settle_release(cleanup.handoff(), release, ContextSettlement::Applied)
            .expect("settle release");
        let retired_handoff = fixture
            .registry
            .reap_terminal(cleanup.handoff())
            .expect("reap handoff");
        fixture
            .workflow
            .observe_cleanup_terminal(retired_handoff)
            .expect("handoff released");
        assert_eq!(
            fixture.workflow.state(),
            ContextSignInHandoffState::Cancelled
        );
    }

    #[test]
    fn workflow_debug_is_redacted_and_never_exposes_origin_or_profile() {
        let fixture = fixture(ContextSignInHandoffPlatform::WindowsCookieBridge);
        let debug = format!("{:?}", fixture.workflow);
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("example.test"));
        assert!(!debug.contains("ProfileId"));
    }
}
